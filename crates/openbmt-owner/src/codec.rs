//! The queue's snapshot, `OBM4` (AS-65).
//!
//! ```text
//! "OBM4" | 1 | wallet ID (32) | owner (32) | Key app (32)
//!   | fee payer u8 | packing u8 | minimum confirmations u32
//!   | next number u32
//!   | known settings: flag, then length u32 and the `OBMS` bytes
//!   | sales | refunds | cash-outs | batches | settings | pairings
//!   | open sessions | finished objects | issued ranges
//! ```
//!
//! Every list is a u32 count and then its entries in ascending order of
//! their key (the issued ranges: counter, start, count and the time the
//! batch was signed, ascending by start and never overlapping, OC-10, OC-11,
//! OC-22). Decoding is strict (AS-03 in the file's rules, AS-65 here): a
//! snapshot is accepted only if encoding what it describes gives the same
//! bytes.

use std::collections::{BTreeMap, BTreeSet};

use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::{OutPoint, ScriptBuf, Transaction, Txid};
use openbmt_core::batch::{BatchBuilder, SignedBatch};
use openbmt_core::identity::Sig;
use openbmt_core::pairing::{check_request, PairOffer, PairRequest};
use openbmt_core::settings::{SettingsError, SignedSettings};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_session::{label_text_ok, reason_text_ok, Request, ShopValue};
use openbmt_spend::dust_limit;

use crate::error::{Refusal, RestoreError};
use crate::state::{outputs_fit, Frozen, OpenSession, Owner, SaleEntry, State};
use crate::types::{
    CashAmount, CashOut, Deliverable, Delivery, FeePayer, IssuedRange, Options, QueuedBatch,
    QueuedPairing, QueuedSettings, Refund, RefundPacking, SaleFacts, SaleKey, Stage,
};
use crate::{
    MAGIC_OWNER_QUEUE, MAX_BATCHES, MAX_CASH_OUTS, MAX_CONFLICT_OUTPOINTS, MAX_ISSUED_RANGES,
    MAX_ITEMS_PER_SESSION, MAX_OPEN_SESSIONS, MAX_OUTBOX, MAX_PAIRINGS, MAX_REFUNDS,
    MAX_REFUNDS_PER_TX, MAX_SALES, MAX_SESSION_REQUEST, MAX_SETTINGS, MAX_SNAPSHOT_BYTES,
};

fn bad(why: &'static str) -> DecodeError {
    DecodeError::NonCanonical(why)
}

// ---- writing --------------------------------------------------------------------

fn put_u32(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes());
}

fn put_text(out: &mut Vec<u8>, s: &str) {
    out.push(u8::try_from(s.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(s.as_bytes());
}

fn put_script(out: &mut Vec<u8>, s: &ScriptBuf) {
    out.push(u8::try_from(s.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(s.as_bytes());
}

fn put_blob(out: &mut Vec<u8>, bytes: &[u8]) {
    put_u32(out, bytes.len());
    out.extend_from_slice(bytes);
}

fn put_txid(out: &mut Vec<u8>, t: &Txid) {
    out.extend_from_slice(&t.to_byte_array());
}

fn put_ids(out: &mut Vec<u8>, ids: &[u32]) {
    put_u32(out, ids.len());
    for id in ids {
        out.extend_from_slice(&id.to_le_bytes());
    }
}

/// AS-68: outpoints, a u32 count and then each as transaction ID and index.
fn put_outpoints(out: &mut Vec<u8>, list: &[OutPoint]) {
    put_u32(out, list.len());
    for o in list {
        put_txid(out, &o.txid);
        out.extend_from_slice(&o.vout.to_le_bytes());
    }
}

fn put_stage(out: &mut Vec<u8>, stage: &Stage) {
    match stage {
        Stage::Queued => out.push(0),
        Stage::InSession { session } => {
            out.push(1);
            out.extend_from_slice(session);
        }
        Stage::Returned { reason } => {
            out.push(2);
            put_text(out, reason);
        }
        Stage::Signed { txid } => {
            out.push(3);
            put_txid(out, txid);
        }
        Stage::Sent { txid } => {
            out.push(4);
            put_txid(out, txid);
        }
        Stage::Paid { txid } => {
            out.push(5);
            put_txid(out, txid);
        }
    }
}

fn put_frozen(out: &mut Vec<u8>, number: u32, f: &Frozen) {
    out.extend_from_slice(&number.to_le_bytes());
    match f {
        Frozen::Spend {
            txid,
            refunds,
            cash_out,
            payments,
            change_index,
            change_position,
        } => {
            out.push(0);
            put_txid(out, txid);
            put_ids(out, refunds);
            match cash_out {
                Some(n) => {
                    out.push(1);
                    out.extend_from_slice(&n.to_le_bytes());
                }
                None => out.push(0),
            }
            put_u32(out, payments.len());
            for (script, sats) in payments {
                put_script(out, script);
                out.extend_from_slice(&sats.to_le_bytes());
            }
            out.extend_from_slice(&change_index.to_le_bytes());
            match change_position {
                Some(p) => {
                    out.push(1);
                    out.extend_from_slice(&p.to_le_bytes());
                }
                None => out.push(0),
            }
        }
        Frozen::Batch(n) => {
            out.push(1);
            out.extend_from_slice(&n.to_le_bytes());
        }
        Frozen::Settings(n) => {
            out.push(2);
            out.extend_from_slice(&n.to_le_bytes());
        }
        Frozen::Pairing(n) => {
            out.push(3);
            out.extend_from_slice(&n.to_le_bytes());
        }
        Frozen::Health => out.push(4),
    }
}

/// AS-65: the snapshot of `state`.
pub(crate) fn encode(owner: &Owner, st: &State) -> Result<Vec<u8>, Refusal> {
    let mut out = wire::start(&MAGIC_OWNER_QUEUE, 1024);
    out.extend_from_slice(&owner.wallet_id);
    out.extend_from_slice(&owner.identity.device_id().to_bytes());
    out.extend_from_slice(&owner.roots.approver.to_bytes());
    out.push(match st.options.fee_payer {
        FeePayer::Shop => 0,
        FeePayer::Customer => 1,
    });
    out.push(match st.options.packing {
        RefundPacking::OneTransaction => 0,
        RefundPacking::OneEach => 1,
    });
    out.extend_from_slice(&st.options.min_confirmations.to_le_bytes());
    out.extend_from_slice(&st.next_id.to_le_bytes());
    match &st.known {
        Some(k) => {
            out.push(1);
            put_blob(&mut out, &k.to_bytes());
        }
        None => out.push(0),
    }
    put_u32(&mut out, st.sales.len());
    for (key, entry) in &st.sales {
        out.extend_from_slice(&key.counter.to_bytes());
        out.extend_from_slice(&key.index.to_le_bytes());
        out.extend_from_slice(&entry.facts.received_sats.to_le_bytes());
        out.push(u8::from(entry.facts.reached_refund_depth));
        out.extend_from_slice(&entry.archived_paid.to_le_bytes());
    }
    put_u32(&mut out, st.refunds.len());
    for r in &st.refunds {
        out.extend_from_slice(&r.id.to_le_bytes());
        out.extend_from_slice(&r.sale.counter.to_bytes());
        out.extend_from_slice(&r.sale.index.to_le_bytes());
        put_text(&mut out, &r.cashier);
        put_script(&mut out, &r.script);
        out.extend_from_slice(&r.sats.to_le_bytes());
        match &r.shop_value {
            Some(v) => {
                out.push(1);
                out.extend_from_slice(&v.minor.to_le_bytes());
                out.extend_from_slice(&v.currency);
            }
            None => out.push(0),
        }
        put_outpoints(&mut out, &r.conflict);
        put_stage(&mut out, &r.stage);
    }
    put_u32(&mut out, st.cash_outs.len());
    for c in &st.cash_outs {
        out.extend_from_slice(&c.id.to_le_bytes());
        put_script(&mut out, &c.script);
        match c.amount {
            CashAmount::Sats(n) => {
                out.push(0);
                out.extend_from_slice(&n.to_le_bytes());
            }
            CashAmount::All => out.push(1),
        }
        put_text(&mut out, &c.label);
        put_outpoints(&mut out, &c.conflict);
        put_stage(&mut out, &c.stage);
    }
    put_u32(&mut out, st.batches.len());
    for b in &st.batches {
        out.extend_from_slice(&b.id.to_le_bytes());
        // The batch is held as a signed batch with no Key app signature.
        let signed = SignedBatch {
            batch: b.batch.clone(),
            owner_sig: b.owner_sig,
            approver_sig: Sig([0; 64]),
        };
        if b.batch.scripts.iter().any(|s| s.len() != 34) {
            return Err(Refusal::BatchNotDerived);
        }
        put_blob(&mut out, &signed.to_bytes());
        put_stage(&mut out, &b.stage);
    }
    put_u32(&mut out, st.settings.len());
    for s in &st.settings {
        out.extend_from_slice(&s.id.to_le_bytes());
        put_blob(&mut out, &s.signed.to_bytes());
        put_stage(&mut out, &s.stage);
    }
    put_u32(&mut out, st.pairings.len());
    for p in &st.pairings {
        out.extend_from_slice(&p.id.to_le_bytes());
        put_blob(&mut out, &p.request.to_bytes());
        put_stage(&mut out, &p.stage);
    }
    put_u32(&mut out, st.sessions.len());
    for s in &st.sessions {
        out.extend_from_slice(&s.id);
        out.extend_from_slice(&s.created_at.to_le_bytes());
        put_blob(&mut out, &s.request);
        put_u32(&mut out, s.frozen.len());
        for (number, f) in &s.frozen {
            put_frozen(&mut out, *number, f);
        }
    }
    put_u32(&mut out, st.outbox.len());
    for d in &st.outbox {
        out.extend_from_slice(&d.id.to_le_bytes());
        match &d.what {
            Delivery::Transaction {
                txid,
                tx,
                refunds,
                cash_out,
            } => {
                out.push(0);
                put_txid(&mut out, txid);
                put_blob(&mut out, tx);
                put_ids(&mut out, refunds);
                match cash_out {
                    Some(n) => {
                        out.push(1);
                        out.extend_from_slice(&n.to_le_bytes());
                    }
                    None => out.push(0),
                }
            }
            Delivery::Batch(b) => {
                out.push(1);
                put_blob(&mut out, b);
            }
            Delivery::Settings(b) => {
                out.push(2);
                put_blob(&mut out, b);
            }
            Delivery::Pairing(b) => {
                out.push(3);
                put_blob(&mut out, b);
            }
        }
    }
    put_u32(&mut out, st.issued.len());
    for range in &st.issued {
        out.extend_from_slice(&range.counter.to_bytes());
        out.extend_from_slice(&range.start.to_le_bytes());
        out.extend_from_slice(&range.count.to_le_bytes());
        out.extend_from_slice(&range.signed_at.to_le_bytes());
    }
    if out.len() > MAX_SNAPSHOT_BYTES {
        return Err(Refusal::SnapshotTooLarge);
    }
    Ok(out)
}

// ---- reading --------------------------------------------------------------------

fn read_text(r: &mut Reader<'_>, ok: fn(&str) -> bool) -> Result<String, DecodeError> {
    let n = usize::from(r.u8()?);
    let s = std::str::from_utf8(r.take(n)?)
        .map_err(|_| bad("text is not UTF-8"))?
        .to_owned();
    if ok(&s) {
        Ok(s)
    } else {
        Err(bad("text breaks the text rule"))
    }
}

fn read_script(r: &mut Reader<'_>) -> Result<ScriptBuf, DecodeError> {
    let n = usize::from(r.u8()?);
    if n > 34 {
        return Err(bad("script is longer than any allowed kind"));
    }
    let script = ScriptBuf::from_bytes(r.take(n)?.to_vec());
    dust_limit(&script).ok_or(bad("script is not of an allowed kind"))?;
    Ok(script)
}

fn read_blob<'a>(r: &mut Reader<'a>, max: usize) -> Result<&'a [u8], DecodeError> {
    let n = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
    if n > max {
        return Err(DecodeError::TooLarge);
    }
    r.take(n)
}

fn read_txid(r: &mut Reader<'_>) -> Result<Txid, DecodeError> {
    Ok(Txid::from_byte_array(r.array()?))
}

fn read_ids(r: &mut Reader<'_>, max: usize) -> Result<Vec<u32>, DecodeError> {
    let n = r.count(max, 4)?;
    (0..n).map(|_| r.u32_le()).collect()
}

/// AS-68: outpoints in strictly ascending order.
fn read_outpoints(r: &mut Reader<'_>) -> Result<Vec<OutPoint>, DecodeError> {
    let n = r.count(MAX_CONFLICT_OUTPOINTS, 36)?;
    let mut list: Vec<OutPoint> = Vec::with_capacity(n);
    for _ in 0..n {
        let txid = read_txid(r)?;
        let o = OutPoint {
            txid,
            vout: r.u32_le()?,
        };
        if list.last().is_some_and(|l| *l >= o) {
            return Err(bad("outpoints are not in ascending order"));
        }
        list.push(o);
    }
    Ok(list)
}

fn read_stage(r: &mut Reader<'_>) -> Result<Stage, DecodeError> {
    Ok(match r.u8()? {
        0 => Stage::Queued,
        1 => Stage::InSession {
            session: r.array()?,
        },
        2 => Stage::Returned {
            reason: read_text(r, reason_text_ok)?,
        },
        3 => Stage::Signed {
            txid: read_txid(r)?,
        },
        4 => Stage::Sent {
            txid: read_txid(r)?,
        },
        5 => Stage::Paid {
            txid: read_txid(r)?,
        },
        k => return Err(DecodeError::UnknownKind(k)),
    })
}

fn read_opt_id(r: &mut Reader<'_>) -> Result<Option<u32>, DecodeError> {
    if r.flag()? {
        Ok(Some(r.u32_le()?))
    } else {
        Ok(None)
    }
}

fn read_frozen(r: &mut Reader<'_>) -> Result<(u32, Frozen), DecodeError> {
    let number = r.u32_le()?;
    let f = match r.u8()? {
        0 => {
            let txid = read_txid(r)?;
            let refunds = read_ids(r, MAX_REFUNDS_PER_TX)?;
            let cash_out = read_opt_id(r)?;
            let n = r.count(MAX_REFUNDS_PER_TX, 9)?;
            let mut payments = Vec::with_capacity(n);
            for _ in 0..n {
                let script = read_script(r)?;
                payments.push((script, r.u64_le()?));
            }
            let change_index = r.u32_le()?;
            let change_position = read_opt_id(r)?;
            if change_position.is_some_and(|p| usize::try_from(p).map_or(true, |p| p > n)) {
                return Err(bad("a change position is past the outputs"));
            }
            Frozen::Spend {
                txid,
                refunds,
                cash_out,
                payments,
                change_index,
                change_position,
            }
        }
        1 => Frozen::Batch(r.u32_le()?),
        2 => Frozen::Settings(r.u32_le()?),
        3 => Frozen::Pairing(r.u32_le()?),
        4 => Frozen::Health,
        k => return Err(DecodeError::UnknownKind(k)),
    };
    Ok((number, f))
}

fn ascending<T>(items: &[T], key: impl Fn(&T) -> u32) -> bool {
    items.windows(2).all(|w| match w {
        [a, b] => key(a) < key(b),
        _ => true,
    })
}

/// AS-65: read a snapshot back, strictly.
pub(crate) fn decode(owner: &Owner, bytes: &[u8]) -> Result<State, RestoreError> {
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(RestoreError::Decode(DecodeError::TooLarge));
    }
    let d = RestoreError::Decode;
    let mut r = Reader::framed(bytes, &MAGIC_OWNER_QUEUE).map_err(d)?;
    if r.array::<32>().map_err(d)? != owner.wallet_id {
        return Err(RestoreError::WrongWallet);
    }
    if r.array::<32>().map_err(d)? != owner.identity.device_id().to_bytes() {
        return Err(RestoreError::WrongOwner);
    }
    if r.array::<32>().map_err(d)? != owner.roots.approver.to_bytes() {
        return Err(RestoreError::WrongApprover);
    }
    let fee_payer = match r.u8().map_err(d)? {
        0 => FeePayer::Shop,
        1 => FeePayer::Customer,
        k => return Err(d(DecodeError::UnknownKind(k))),
    };
    let packing = match r.u8().map_err(d)? {
        0 => RefundPacking::OneTransaction,
        1 => RefundPacking::OneEach,
        k => return Err(d(DecodeError::UnknownKind(k))),
    };
    let options = Options {
        fee_payer,
        packing,
        min_confirmations: r.u32_le().map_err(d)?,
    };
    let mut st = State::new(options);
    st.next_id = r.u32_le().map_err(d)?;
    if st.next_id == 0 {
        return Err(d(bad("the numbering starts at 1")));
    }
    if r.flag().map_err(d)? {
        let blob = read_blob(&mut r, 1 << 20).map_err(d)?;
        let signed = SignedSettings::from_bytes(blob).map_err(d)?;
        // The owner's signature and the values; the Key app's signature is
        // not needed to know a tightening.
        match signed.verify(&owner.roots, None) {
            Ok(_) | Err(SettingsError::ApproverRequired) => {}
            Err(_) => {
                return Err(RestoreError::Inconsistent(
                    "the known settings do not verify",
                ))
            }
        }
        st.known = Some(signed);
    }

    let n = r.count(MAX_SALES, 49).map_err(d)?;
    let mut last: Option<SaleKey> = None;
    for _ in 0..n {
        let counter = r.device_id().map_err(d)?;
        let key = SaleKey {
            counter,
            index: r.u32_le().map_err(d)?,
        };
        if last.is_some_and(|l| l >= key) {
            return Err(d(bad("sales are not in ascending order")));
        }
        last = Some(key);
        let received_sats = r.u64_le().map_err(d)?;
        let reached_refund_depth = r.flag().map_err(d)?;
        let archived_paid = r.u64_le().map_err(d)?;
        st.sales.insert(
            key,
            SaleEntry {
                facts: SaleFacts {
                    received_sats,
                    reached_refund_depth,
                },
                archived_paid,
            },
        );
    }

    let n = r.count(MAX_REFUNDS, 50).map_err(d)?;
    for _ in 0..n {
        let id = r.u32_le().map_err(d)?;
        let counter = r.device_id().map_err(d)?;
        let index = r.u32_le().map_err(d)?;
        let cashier = read_text(&mut r, label_text_ok).map_err(d)?;
        let script = read_script(&mut r).map_err(d)?;
        let sats = r.u64_le().map_err(d)?;
        let shop_value = if r.flag().map_err(d)? {
            let minor = r.u64_le().map_err(d)?;
            let currency: [u8; 3] = r.array().map_err(d)?;
            if !currency.iter().all(u8::is_ascii_uppercase) {
                return Err(d(bad("currency code is not three capital letters")));
            }
            Some(ShopValue { minor, currency })
        } else {
            None
        };
        let conflict = read_outpoints(&mut r).map_err(d)?;
        let stage = read_stage(&mut r).map_err(d)?;
        let dust = dust_limit(&script).unwrap_or(u64::MAX);
        if sats < dust {
            return Err(d(bad("a refund is below the dust limit")));
        }
        st.refunds.push(Refund {
            id,
            sale: SaleKey { counter, index },
            cashier,
            script,
            sats,
            shop_value,
            conflict,
            stage,
        });
    }

    let n = r.count(MAX_CASH_OUTS, 8).map_err(d)?;
    for _ in 0..n {
        let id = r.u32_le().map_err(d)?;
        let script = read_script(&mut r).map_err(d)?;
        let amount = match r.u8().map_err(d)? {
            0 => CashAmount::Sats(r.u64_le().map_err(d)?),
            1 => CashAmount::All,
            k => return Err(d(DecodeError::UnknownKind(k))),
        };
        let label = read_text(&mut r, label_text_ok).map_err(d)?;
        let conflict = read_outpoints(&mut r).map_err(d)?;
        let stage = read_stage(&mut r).map_err(d)?;
        st.cash_outs.push(CashOut {
            id,
            script,
            amount,
            label,
            conflict,
            stage,
        });
    }

    let n = r.count(MAX_BATCHES, 9).map_err(d)?;
    for _ in 0..n {
        let id = r.u32_le().map_err(d)?;
        let blob = read_blob(&mut r, 1 << 22).map_err(d)?;
        let signed = SignedBatch::from_bytes(blob).map_err(d)?;
        if signed.approver_sig.0 != [0; 64] {
            return Err(d(bad("a queued batch carries a Key app signature")));
        }
        let derived = BatchBuilder::new(owner.policy)
            .derive(
                &owner.wallet,
                signed.batch.counter,
                signed.batch.start,
                signed.batch.count,
            )
            .map_err(|_| RestoreError::Inconsistent("a queued batch is not derived"))?;
        if derived != signed.batch {
            return Err(RestoreError::Inconsistent("a queued batch is not derived"));
        }
        if !signed
            .batch
            .owner_signature_valid(&owner.roots.owner, &signed.owner_sig)
        {
            return Err(RestoreError::Inconsistent(
                "a queued batch does not carry the owner's signature",
            ));
        }
        let stage = read_stage(&mut r).map_err(d)?;
        st.batches.push(QueuedBatch {
            id,
            batch: signed.batch,
            owner_sig: signed.owner_sig,
            stage,
        });
    }

    let n = r.count(MAX_SETTINGS, 9).map_err(d)?;
    for _ in 0..n {
        let id = r.u32_le().map_err(d)?;
        let blob = read_blob(&mut r, 1 << 20).map_err(d)?;
        let signed = SignedSettings::from_bytes(blob).map_err(d)?;
        if signed.approver_sig.is_some() {
            return Err(d(bad("queued settings carry a Key app signature")));
        }
        match signed.verify(&owner.roots, None) {
            Ok(_) | Err(SettingsError::ApproverRequired) => {}
            Err(_) => return Err(RestoreError::Inconsistent("queued settings do not verify")),
        }
        let stage = read_stage(&mut r).map_err(d)?;
        st.settings.push(QueuedSettings { id, signed, stage });
    }

    let offer = PairOffer::new(
        owner.identity.device_id(),
        owner.roots.approver,
        owner.wallet_id,
        owner.wallet.network(),
    )
    .map_err(|_| RestoreError::Inconsistent("the pairing offer"))?;
    let n = r.count(MAX_PAIRINGS, 9).map_err(d)?;
    for _ in 0..n {
        let id = r.u32_le().map_err(d)?;
        let blob = read_blob(&mut r, 1 << 12).map_err(d)?;
        let request = PairRequest::from_bytes(blob).map_err(d)?;
        check_request(&offer, &request)
            .map_err(|_| RestoreError::Inconsistent("a queued pairing request is refused"))?;
        let stage = read_stage(&mut r).map_err(d)?;
        st.pairings.push(QueuedPairing { id, request, stage });
    }

    let n = r.count(MAX_OPEN_SESSIONS, 44).map_err(d)?;
    for _ in 0..n {
        let id: [u8; 32] = r.array().map_err(d)?;
        let created_at = r.u64_le().map_err(d)?;
        let request = read_blob(&mut r, MAX_SESSION_REQUEST).map_err(d)?.to_vec();
        let m = r.count(MAX_ITEMS_PER_SESSION, 5).map_err(d)?;
        let mut frozen = Vec::with_capacity(m);
        for _ in 0..m {
            frozen.push(read_frozen(&mut r).map_err(d)?);
        }
        st.sessions.push(OpenSession {
            id,
            created_at,
            request,
            frozen,
        });
    }

    let n = r.count(MAX_OUTBOX, 9).map_err(d)?;
    for _ in 0..n {
        let id = r.u32_le().map_err(d)?;
        let what = match r.u8().map_err(d)? {
            0 => {
                let txid = read_txid(&mut r).map_err(d)?;
                let raw = read_blob(&mut r, 1 << 20).map_err(d)?;
                let tx: Transaction =
                    deserialize(raw).map_err(|_| d(bad("a transaction does not parse")))?;
                if tx.compute_txid() != txid || serialize(&tx) != raw {
                    return Err(RestoreError::Inconsistent(
                        "a transaction is not the one its ID names",
                    ));
                }
                let refunds = read_ids(&mut r, MAX_REFUNDS_PER_TX).map_err(d)?;
                let cash_out = read_opt_id(&mut r).map_err(d)?;
                Delivery::Transaction {
                    txid,
                    tx: raw.to_vec(),
                    refunds,
                    cash_out,
                }
            }
            1 => Delivery::Batch(read_blob(&mut r, 1 << 22).map_err(d)?.to_vec()),
            2 => Delivery::Settings(read_blob(&mut r, 1 << 20).map_err(d)?.to_vec()),
            3 => Delivery::Pairing(read_blob(&mut r, 1 << 12).map_err(d)?.to_vec()),
            k => return Err(d(DecodeError::UnknownKind(k))),
        };
        st.outbox.push(Deliverable { id, what });
    }

    let n = r.count(MAX_ISSUED_RANGES, 48).map_err(d)?;
    let mut next_free: u64 = 0;
    for _ in 0..n {
        let counter = r.device_id().map_err(d)?;
        let start = r.u32_le().map_err(d)?;
        let count = r.u32_le().map_err(d)?;
        let signed_at = r.u64_le().map_err(d)?;
        let range = IssuedRange {
            start,
            count,
            counter,
            signed_at,
        };
        if count == 0 || range.end() > u64::from(u32::MAX) {
            return Err(d(bad("an issued range is empty or past the last index")));
        }
        if u64::from(start) < next_free {
            return Err(d(bad("issued ranges overlap or are not in order")));
        }
        next_free = range.end();
        st.issued.push(range);
    }
    r.finish().map_err(d)?;

    check_consistency(owner, &st)?;
    // Canonical form: encoding what was read gives the same bytes.
    match encode(owner, &st) {
        Ok(again) if again == bytes => Ok(st),
        _ => Err(d(bad("not what encoding the snapshot would give"))),
    }
}

/// AS-65: the parts of a snapshot must agree with each other.
fn check_consistency(owner: &Owner, st: &State) -> Result<(), RestoreError> {
    let bad = RestoreError::Inconsistent;
    if !(ascending(&st.refunds, |r| r.id)
        && ascending(&st.cash_outs, |r| r.id)
        && ascending(&st.batches, |r| r.id)
        && ascending(&st.settings, |r| r.id)
        && ascending(&st.pairings, |r| r.id)
        && ascending(&st.outbox, |r| r.id))
    {
        return Err(bad("numbers are not in ascending order"));
    }
    // Numbers are unique across every list and below the next number.
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let ids = st
        .refunds
        .iter()
        .map(|r| r.id)
        .chain(st.cash_outs.iter().map(|r| r.id))
        .chain(st.batches.iter().map(|r| r.id))
        .chain(st.settings.iter().map(|r| r.id))
        .chain(st.pairings.iter().map(|r| r.id))
        .chain(st.outbox.iter().map(|r| r.id));
    for id in ids {
        if id >= st.next_id || !seen.insert(id) {
            return Err(bad("a number is used twice or is not below the next"));
        }
    }
    // OC-10: every batch the owner holds lies in a range issued for its
    // counter. Ranges outlive batches, so the other way round is not asked.
    let covered = |counter: openbmt_core::identity::DeviceId, start: u32, count: u32| {
        st.issued
            .iter()
            .any(|r| r.counter == counter && r.start == start && r.count == count)
    };
    let waiting_batches: Vec<SignedBatch> = st
        .outbox
        .iter()
        .filter_map(|d| match &d.what {
            Delivery::Batch(bytes) => SignedBatch::from_bytes(bytes).ok(),
            _ => None,
        })
        .collect();
    if !st
        .batches
        .iter()
        .map(|q| &q.batch)
        .chain(waiting_batches.iter().map(|b| &b.batch))
        .all(|b| covered(b.counter, b.start, b.count))
    {
        return Err(bad("a batch is not among the issued ranges"));
    }
    // Entries that are not refunds or cash-outs have only the first three
    // stages.
    let plain = |s: &Stage| {
        matches!(
            s,
            Stage::Queued | Stage::InSession { .. } | Stage::Returned { .. }
        )
    };
    if !(st.batches.iter().all(|b| plain(&b.stage))
        && st.settings.iter().all(|b| plain(&b.stage))
        && st.pairings.iter().all(|b| plain(&b.stage)))
    {
        return Err(bad(
            "a batch, settings or pairing has a stage it cannot have",
        ));
    }
    // Stages that name a session agree with the sessions.
    let mut in_session: BTreeMap<u32, [u8; 32]> = BTreeMap::new();
    let stages = st
        .refunds
        .iter()
        .map(|r| (r.id, &r.stage))
        .chain(st.cash_outs.iter().map(|r| (r.id, &r.stage)))
        .chain(st.batches.iter().map(|r| (r.id, &r.stage)))
        .chain(st.settings.iter().map(|r| (r.id, &r.stage)))
        .chain(st.pairings.iter().map(|r| (r.id, &r.stage)));
    for (id, stage) in stages {
        if let Stage::InSession { session } = stage {
            in_session.insert(id, *session);
        }
    }
    let mut frozen_refs: BTreeMap<u32, [u8; 32]> = BTreeMap::new();
    let mut session_ids: BTreeSet<[u8; 32]> = BTreeSet::new();
    for s in &st.sessions {
        if !session_ids.insert(s.id) {
            return Err(bad("a session is open twice"));
        }
        let request = Request::from_bytes(&s.request)
            .map_err(|_| bad("an open session's request does not decode"))?;
        if request.verify(&owner.identity.device_id()).is_err()
            || *request.wallet() != owner.wallet_id
            || *request.session_id() != s.id
        {
            return Err(bad("an open session's request is not this owner's"));
        }
        if request.items().len() != s.frozen.len() {
            return Err(bad("an open session froze another number of items"));
        }
        for (asked, (number, f)) in request.items().iter().zip(&s.frozen) {
            use openbmt_session::ItemKind as K;
            let fits = matches!(
                (asked.item.kind(), f),
                (K::Spend, Frozen::Spend { .. })
                    | (K::BatchRefill, Frozen::Batch(_))
                    | (K::Settings, Frozen::Settings(_))
                    | (K::Pairing, Frozen::Pairing(_))
                    | (K::HealthCheck, Frozen::Health)
            );
            if asked.number != *number || !fits {
                return Err(bad("an open session's frozen item is not its request's"));
            }
            // AS-67: what was frozen for a transaction is the transaction
            // in the request.
            if let (
                openbmt_session::Item::Spend { psbt, .. },
                Frozen::Spend {
                    txid,
                    payments,
                    change_position,
                    ..
                },
            ) = (&asked.item, f)
            {
                let parsed = bitcoin::psbt::Psbt::deserialize(psbt)
                    .map_err(|_| bad("an open session's transaction does not parse"))?;
                let tx = &parsed.unsigned_tx;
                let same = tx.compute_txid() == *txid
                    && outputs_fit(&tx.output, payments, *change_position);
                if !same {
                    return Err(bad("a frozen transaction is not the one in the request"));
                }
            }
            let refs: Vec<u32> = match f {
                Frozen::Spend {
                    refunds, cash_out, ..
                } => refunds.iter().copied().chain(*cash_out).collect(),
                Frozen::Batch(n) | Frozen::Settings(n) | Frozen::Pairing(n) => vec![*n],
                Frozen::Health => Vec::new(),
            };
            for n in refs {
                if frozen_refs.insert(n, s.id).is_some() {
                    return Err(bad("an entry is frozen into two items"));
                }
            }
        }
    }
    if in_session != frozen_refs {
        return Err(bad("the stages and the open sessions disagree"));
    }
    // Signed and sent entries are held by a transaction that waits.
    let mut held: BTreeMap<u32, Txid> = BTreeMap::new();
    for d in &st.outbox {
        if let Delivery::Transaction {
            txid,
            refunds,
            cash_out,
            ..
        } = &d.what
        {
            for n in refunds.iter().copied().chain(*cash_out) {
                if held.insert(n, *txid).is_some() {
                    return Err(bad("an entry is held by two transactions"));
                }
            }
        }
    }
    let mut waiting: BTreeMap<u32, Txid> = BTreeMap::new();
    let stages = st
        .refunds
        .iter()
        .map(|r| (r.id, &r.stage))
        .chain(st.cash_outs.iter().map(|r| (r.id, &r.stage)));
    for (id, stage) in stages {
        if let Stage::Signed { txid } | Stage::Sent { txid } = stage {
            waiting.insert(id, *txid);
        }
    }
    if waiting != held {
        return Err(bad("signed entries and waiting transactions disagree"));
    }
    Ok(())
}

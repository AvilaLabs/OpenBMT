// Host-side feasibility probe against the real OpenBMT counter library.
// All payment evidence below is fabricated local transaction data, not settlement.
use std::collections::BTreeMap;

use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::Hash;
use bitcoin::transaction::Version;
use bitcoin::{
    absolute::LockTime, Amount as Sats, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut,
    Txid, WScriptHash,
};
use openbmt_core::batch::{AddressBatch, BatchPolicy, PinnedRoots, SignedBatch};
use openbmt_core::identity::Identity;
use openbmt_core::policy::Heartbeat;
use openbmt_core::settings::Settings;
use openbmt_counter::{
    Amount, Counter, Currency, Facts, OpenFacts, Pairing, Rate, RateSource, Report, Slot, Store,
    Stored, TillRequest,
};
use openbmt_watch::{HeaderChain, Health, PeerAddr, Source};

const NOW: u64 = 1_000_000;
const SATS_PER_USD: u64 = 1_000;

#[derive(Default)]
struct SnapshotStore {
    slots: BTreeMap<Slot, Vec<u8>>,
    records: Vec<Vec<u8>>,
}
impl Store for SnapshotStore {
    type Error = &'static str;
    fn put(&mut self, slot: Slot, bytes: &[u8]) -> Result<(), Self::Error> {
        self.slots.insert(slot, bytes.to_vec());
        Ok(())
    }
    fn delete(&mut self, slot: Slot) -> Result<(), Self::Error> {
        self.slots.remove(&slot);
        Ok(())
    }
    fn append_record(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.records.push(bytes.to_vec());
        Ok(())
    }
    fn read_records(&self, from: u64, max: usize) -> Result<Vec<Vec<u8>>, Self::Error> {
        Ok(self
            .records
            .iter()
            .skip(usize::try_from(from).unwrap_or(usize::MAX))
            .take(max)
            .cloned()
            .collect())
    }
}
impl SnapshotStore {
    fn snapshot(&self) -> Stored<'_> {
        Stored {
            batches: self.slots.get(&Slot::Batches).map(Vec::as_slice),
            ranges: None,
            exposure: self.slots.get(&Slot::Exposure).map(Vec::as_slice),
            pause: self.slots.get(&Slot::Pause).map(Vec::as_slice),
            desk: self.slots.get(&Slot::Desk).map(Vec::as_slice),
            bills: self.slots.get(&Slot::Bills).map(Vec::as_slice),
            sales: self
                .slots
                .iter()
                .filter_map(|(s, b)| match s {
                    Slot::Sale(i) => Some((*i, b.as_slice())),
                    _ => None,
                })
                .collect(),
        }
    }
}

struct EmptyChain;
impl HeaderChain for EmptyChain {
    fn height_of(&self, _: &bitcoin::BlockHash) -> Option<u32> {
        None
    }
    fn tip_height(&self) -> u32 {
        100
    }
    fn tip_time(&self) -> u64 {
        NOW
    }
}

fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}
fn settings() -> Settings {
    let mut s = Settings::defaults(SATS_PER_USD);
    s.customer_wifi = false;
    s.min_sale_sats = 0;
    s.devices.insert(id(3).device_id());
    s.counters.insert(id(3).device_id(), 10_000);
    s
}
fn pairing() -> Pairing {
    Pairing {
        roots: PinnedRoots::new(id(1).device_id(), id(2).device_id()).unwrap(),
        wallet_checksum: [9; 32],
        batch_policy: BatchPolicy { min_count: 1 },
    }
}
fn script(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([n; 32]))
}
fn signed_batch(start: u32, count: u32) -> SignedBatch {
    let batch = AddressBatch {
        wallet_checksum: [9; 32],
        counter: id(3).device_id(),
        start,
        count,
        scripts: (start..start + count)
            .map(|i| script(i as u8 + 1))
            .collect(),
    };
    SignedBatch {
        owner_sig: batch.sign(&id(1)),
        approver_sig: batch.sign(&id(2)),
        batch,
    }
}
fn healthy() -> Health {
    Health {
        healthy: true,
        problem: None,
        behind: false,
    }
}
fn open_facts(now: u64) -> OpenFacts {
    OpenFacts {
        health: healthy(),
        rescan_pending: false,
        now,
    }
}
fn rate_source(now: u64) -> RateSource {
    RateSource::new("synthetic-fixed", now).unwrap()
}
fn tx(script: &ScriptBuf, sats: u64, coin: u8) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_byte_array([coin; 32]),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![TxOut {
            value: Sats::from_sat(sats),
            script_pubkey: script.clone(),
        }],
    }
}
fn facts<'a>(chain: &'a EmptyChain, now: u64, remote_order: bool) -> Facts<'a, EmptyChain> {
    Facts {
        chain,
        health: healthy(),
        heartbeat: Heartbeat {
            verified: true,
            age_secs: 0,
        },
        remote_order,
        now,
    }
}
fn sight_three(
    counter: &mut Counter,
    store: &mut SnapshotStore,
    payment: &Transaction,
    now: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    for n in 20..23 {
        let source = Source::Peer(PeerAddr::Ip(format!("{n}.{n}.1.1").parse()?));
        counter.sighting(&serialize(payment), source, now, store)?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut counter = Counter::new(id(3), pairing(), settings());
    let mut store = SnapshotStore::default();
    counter.add_batch(signed_batch(0, 8), &mut store)?;
    let currency = Currency::of_shop(counter.settings().currency);
    let rate = Rate::new(10_000_000).ok_or("rate must be nonzero")?;

    // Direct merchant path: stable Opened.index, no Android or Box dependency.
    let merchant = counter.open(
        Amount(500),
        currency,
        rate,
        &rate_source(NOW),
        open_facts(NOW),
        &mut store,
    )?;
    let merchant_payment = tx(&merchant.script, merchant.needs_sats, 31);
    sight_three(&mut counter, &mut store, &merchant_payment, NOW + 10)?;
    let chain = EmptyChain;
    let pending = counter.judge(merchant.index, &facts(&chain, NOW + 10, false), &mut store)?;
    let first_records = store.records.len();
    sight_three(&mut counter, &mut store, &merchant_payment, NOW + 10)?;
    let retry_records = store.records.len();
    let after_wait = counter.judge(merchant.index, &facts(&chain, NOW + 18, false), &mut store)?;
    println!("merchant sale={} pending={:?} accepted_after_conflict_wait={:?} retry_records_before={} after={}", merchant.index, pending.state, after_wait.state, first_records, retry_records);

    // Exercise the only current external-origin record: the Box till request contract.
    let till_id = [0x42; 16];
    counter
        .till_take(
            TillRequest {
                sale: till_id,
                amount: Amount(700),
                currency,
                reference: "web-invoice-204".into(),
                lapses_at: NOW + 600,
            },
            NOW,
        )
        .map_err(|e| std::io::Error::other(format!("till_take: {e:?}")))?;
    let web = counter
        .till_start(
            &till_id,
            rate,
            &rate_source(NOW),
            open_facts(NOW),
            None,
            &mut store,
        )
        .map_err(|e| std::io::Error::other(format!("till_start: {e:?}")))?;
    let association_before = counter.sale_till(web.index) == Some(&till_id);
    let mut counter = Counter::load(id(3), pairing(), settings(), store.snapshot(), &store)?;
    let association_after = counter.sale_till(web.index) == Some(&till_id);
    println!(
        "till_association_before_reload={} after_reload={} receive_index={}",
        association_before, association_after, web.index
    );

    // The policy's output is payment acceptance, not a generic fulfillment command.
    let web_payment = tx(&web.script, web.needs_sats, 32);
    sight_three(&mut counter, &mut store, &web_payment, NOW + 30)?;
    let till_pending = counter.judge(web.index, &facts(&chain, NOW + 30, true), &mut store)?;
    let till_accepted = counter.judge(web.index, &facts(&chain, NOW + 38, true), &mut store)?;
    let pos_paid_report = till_accepted
        .reports
        .iter()
        .any(|r| matches!(r, Report::PosPaid { .. }));
    println!("till_origin_sale={} pending={:?} later={:?} emits_pos_paid={} ; no fulfillment acknowledgment API", web.index, till_pending.state, till_accepted.state, pos_paid_report);

    // Cancel + subsequent synthetic payment demonstrates retained late-payment handling.
    let cancelled = counter.open(
        Amount(500),
        currency,
        rate,
        &rate_source(NOW + 40),
        open_facts(NOW + 40),
        &mut store,
    )?;
    counter.cancel(cancelled.index, NOW + 40, &mut store)?;
    let late = tx(&cancelled.script, cancelled.needs_sats, 33);
    sight_three(&mut counter, &mut store, &late, NOW + 41)?;
    let cancelled_view =
        counter.judge(cancelled.index, &facts(&chain, NOW + 41, false), &mut store)?;
    println!(
        "cancel_then_late_payment sale={} state={:?} late_flag={}",
        cancelled.index, cancelled_view.state, cancelled_view.late
    );
    Ok(())
}

//! The owner's intake of refund requests: a counter's records, the owner's
//! verified copy of them, and the owner's queue (`docs/COUNTER-RULES.md`
//! CR-60 to CR-74).

use std::fs;
use std::sync::OnceLock;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::{All, Secp256k1};
use bitcoin::transaction::Version;
use bitcoin::{
    Amount as Sats, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid,
    WPubkeyHash, Witness,
};
use openbmt_core::batch::{BatchBuilder, BatchPolicy};
use openbmt_core::records::{Chain, DeviceList, RecordKind};
use openbmt_core::wallet::{KeySlot, Keychain, WalletDescription};
use openbmt_counter::{Amount, Ask, Currency, Event, RateSource, Role};
use openbmt_owner::{Options, Owner, SaleFacts, SaleKey, Stage};
use openbmt_spend::Coin;

use super::*;
use crate::coins::{CoinView, Receipt};
use crate::copy::{CounterCopy, Stop};
use crate::facts::FactsRefused;
use crate::refunds::{
    Evidence, NoteKind, RefundIntake, Stopped, MAGIC_INTAKE, MAX_NOTES, STATE_FILE,
};

const NET: Network = Network::Regtest;

// ---- the owner's wallet and queue ----------------------------------------------

struct Kit {
    xpubs: [Xpub; 3],
    wallet: WalletDescription,
}

fn secp() -> &'static Secp256k1<All> {
    static CONTEXT: OnceLock<Secp256k1<All>> = OnceLock::new();
    CONTEXT.get_or_init(Secp256k1::new)
}

/// Three keys from fixed seeds, and the wallet they make. Test only.
fn kit() -> &'static Kit {
    static KIT: OnceLock<Kit> = OnceLock::new();
    KIT.get_or_init(|| {
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let mut accounts = Vec::new();
        let mut slots = Vec::new();
        for n in 0..3u8 {
            let master = Xpriv::new_master(NET, &[n + 1; 32]).unwrap();
            let account = master.derive_priv(secp(), &path).unwrap();
            slots.push(KeySlot {
                index: n + 1,
                origin_fingerprint: master.fingerprint(secp()),
                origin_path: path.clone(),
                xpub: Xpub::from_priv(secp(), &account),
            });
            accounts.push(account);
        }
        let wallet = WalletDescription::new(NET, slots.try_into().unwrap()).unwrap();
        let xpubs = [0, 1, 2].map(|n| Xpub::from_priv(secp(), &accounts[n]));
        Kit { xpubs, wallet }
    })
}

/// The tip of the chain the owner's view has checked, and the refund depth.
pub(super) const TIP: u32 = 100;
pub(super) const DEPTH: u32 = 6;

/// An owner that signed one batch of 20 addresses, from index 0, for
/// `counter` (OC-10).
pub(super) fn new_owner_for(counter: DeviceId) -> Owner {
    let mut owner = Owner::new(
        &[1; 32],
        kit().wallet.clone(),
        id(2).device_id(),
        BatchPolicy { min_count: 3 },
        Options::default(),
    )
    .unwrap();
    let batch = BatchBuilder::new(BatchPolicy { min_count: 3 })
        .derive(&kit().wallet, counter, 0, 20)
        .unwrap();
    owner
        .queue_batch(batch, 1_000, |_| Ok::<(), String>(()))
        .unwrap();
    owner
}

pub(super) fn new_owner() -> Owner {
    new_owner_for(counter())
}

/// A coin view that has checked the chain up to [`TIP`] and holds one
/// payment for each `(index, sats, depth)`, the tip block counting as depth
/// 1 (OC-12).
pub(super) fn view_with(paid: &[(u32, u64, u32)]) -> CoinView {
    let mut view = CoinView::new(0);
    view.set_checked(TIP, |_| Ok::<(), ()>(())).unwrap();
    for (n, (index, sats, depth)) in paid.iter().enumerate() {
        let n = u32::try_from(n).unwrap();
        let mut txid = [0u8; 32];
        txid[..4].copy_from_slice(&n.to_le_bytes());
        txid[4..8].copy_from_slice(&index.to_le_bytes());
        assert!(view.add_receipt_for_test(Receipt {
            index: *index,
            outpoint: OutPoint::new(Txid::from_byte_array(txid), 0),
            sats: *sats,
            height: TIP - depth + 1,
            block: bitcoin::BlockHash::from_byte_array([u8::try_from(n).unwrap() + 1; 32]),
        }));
    }
    view
}

/// A coin of `sats` on the receive keychain at `index`, 6 deep.
fn coin(tag: u32, index: u32, sats: u64) -> Coin {
    let mut funding = [0u8; 32];
    funding[..4].copy_from_slice(&tag.to_le_bytes());
    let prev = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_byte_array(funding),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![
            TxOut {
                value: Sats::from_sat(1),
                script_pubkey: dest(250),
            },
            TxOut {
                value: Sats::from_sat(sats),
                script_pubkey: kit().wallet.receive_script(index).unwrap(),
            },
        ],
    };
    Coin {
        outpoint: OutPoint {
            txid: prev.compute_txid(),
            vout: 1,
        },
        prev_tx: prev,
        vout: 1,
        chain: Keychain::Receive,
        index,
        confirmations: 6,
    }
}

/// A destination: a P2WPKH script.
fn dest(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([n; 20]))
}

// ---- the counter's records ------------------------------------------------------

fn usd() -> Currency {
    Currency::new(*b"USD").unwrap()
}

fn opened(index: u32) -> Event {
    Event::Opened {
        index,
        at: T0,
        amount: Amount(500),
        currency: usd(),
        needs_sats: 5_000,
        minor_per_btc: 10_000_000,
        source: RateSource::new("test-fixed", T0).unwrap(),
        by: None,
    }
}

fn confirmed(index: u32, sats: u64) -> Event {
    Event::Confirmed {
        index,
        at: T0 + 10,
        received_sats: sats,
        depth: 1,
    }
}

fn finished(index: u32, sats: u64) -> Event {
    Event::Final {
        index,
        at: T0 + 20,
        received_sats: sats,
        depth: 6,
        first_seen_at: T0,
        valued: Amount(500),
        payments: vec![([1; 32], sats)],
        more: false,
    }
}

fn requested(index: u32, number: u64, sats: u64, destination: ScriptBuf, received: u64) -> Event {
    Event::RefundRequested {
        index,
        at: T0 + 30,
        number,
        role: Role::Cashier,
        name: "ann".to_string(),
        reason: "wrong size".to_string(),
        destination,
        received_sats: received,
        waiting_for_depth: false,
        sats,
        ask: Ask::Sats,
        chain_height: None,
    }
}

fn withdrawn(index: u32, number: u64) -> Event {
    Event::RefundWithdrawn {
        index,
        at: T0 + 40,
        number,
        role: Role::Manager,
        name: "max".to_string(),
    }
}

fn paid_in_cash(index: u32, minor: u64) -> Event {
    Event::RefundPaidInCash {
        index,
        at: T0 + 50,
        role: Role::Cashier,
        name: "ann".to_string(),
        reason: "till".to_string(),
        minor,
        sats: minor * 100,
        chain_height: None,
    }
}

fn devices() -> DeviceList {
    DeviceList::new([counter(), id(1).device_id()])
}

fn key(index: u32) -> SaleKey {
    SaleKey {
        counter: counter(),
        index,
    }
}

type Persist = Box<dyn FnMut(&[u8]) -> Result<(), String>>;

fn ok_persist() -> Persist {
    Box::new(|_| Ok(()))
}

/// The owner's side of one counter: its chain, the owner's verified copy of
/// it, the intake and the owner's queue.
struct Side {
    dir: Dir,
    chain: Chain,
    copy: CounterCopy,
    intake: RefundIntake,
    owner: Owner,
    /// The owner's own view of the chain: the facts of a sale come from it.
    view: CoinView,
}

impl Side {
    /// A side whose view shows sale 0 paid 5,000 sats, at the refund depth.
    fn new() -> Self {
        Self::with_view(view_with(&[(0, 5_000, DEPTH)]))
    }

    fn with_view(view: CoinView) -> Self {
        let dir = Dir::new();
        let (copy, _) = CounterCopy::open(dir.path(), counter(), &devices()).unwrap();
        let intake = RefundIntake::open(dir.path()).unwrap();
        Self {
            dir,
            chain: Chain::new(),
            copy,
            intake,
            owner: new_owner(),
            view,
        }
    }

    /// The counter writes these events and they reach the owner's copy.
    fn push(&mut self, events: &[Event]) {
        let before = self.chain.records().len();
        for e in events {
            self.chain.append(&id(3), e.kind(), e.encode()).unwrap();
        }
        let new = self.chain.records()[before..].to_vec();
        let taken = self.copy.take(new, &devices());
        assert_eq!(taken.stopped, None);
    }

    fn apply(&mut self) -> crate::Applied {
        self.apply_with(&mut ok_persist())
    }

    fn apply_with(&mut self, persist: &mut Persist) -> crate::Applied {
        let evidence = Evidence {
            view: &self.view,
            tip: TIP,
            refund_depth: DEPTH,
        };
        self.intake
            .apply(&mut self.owner, counter(), &self.copy, &evidence, persist)
    }

    /// The intake as a restarted app would open it.
    fn restart_intake(&mut self) {
        self.intake = RefundIntake::open(self.dir.path()).unwrap();
    }
}

// ---- the tests ------------------------------------------------------------------

#[test]
fn cr_60_a_counter_chain_with_a_request_becomes_exactly_one_queued_refund() {
    let mut s = Side::new();
    s.push(&[
        opened(0),
        finished(0, 5_000),
        requested(0, 1, 2_000, dest(9), 5_000),
    ]);
    let done = s.apply();
    assert_eq!((done.queued, done.refused, done.stopped), (1, 0, None));
    let refunds = s.owner.refunds();
    assert_eq!(refunds.len(), 1);
    let r = &refunds[0];
    assert_eq!(r.sale, key(0));
    assert_eq!(r.sats, 2_000);
    assert_eq!(r.script, dest(9));
    assert_eq!(r.cashier, "ann");
    assert_eq!(r.stage, Stage::Queued);
    assert!(r.shop_value.is_none());
    // The owner holds the sale's facts from the same copy: received, and at
    // the refund depth because the chain holds a final record.
    assert_eq!(
        s.owner.sale_facts(&key(0)),
        Some(SaleFacts {
            received_sats: 5_000,
            reached_refund_depth: true
        })
    );
    assert_eq!(s.owner.waiting_for_depth(r.id), Some(false));
    assert_eq!(s.intake.queued_id(&counter(), 1), Some(r.id));
    assert_eq!(s.intake.applied_up_to(&counter()), s.copy.len());
    assert!(s.intake.notes().is_empty());
}

#[test]
fn cr_61_request_numbers_may_skip_and_one_ask_that_stopped_is_one_queued_refund() {
    let mut s = Side::new();
    // What a counter's records hold after a stop between the desk and the
    // record (number 1 never written), a stop between the record and the
    // sale (number 2, adopted by the counter at its next start, so that the
    // cashier is not led to ask again), and a withdrawal whose sale write
    // stopped (number 5 is withdrawn in the records).
    s.push(&[
        opened(0),
        finished(0, 5_000),
        requested(0, 2, 2_000, dest(9), 5_000),
        requested(0, 5, 1_000, dest(8), 5_000),
        withdrawn(0, 5),
    ]);
    let done = s.apply();
    assert_eq!((done.queued, done.refused, done.withdrawn), (2, 0, 1));
    let refunds = s.owner.refunds();
    assert_eq!(refunds.len(), 1, "one ask, one refund");
    assert_eq!(
        (refunds[0].sats, refunds[0].script.clone()),
        (2_000, dest(9))
    );
    assert!(s.intake.queued_id(&counter(), 2).is_some());
    assert!(s.intake.notes().is_empty());
}

#[test]
fn cr_61_a_request_in_the_shops_currency_keeps_its_value_in_the_queue() {
    let mut s = Side::new();
    let mut by_value = requested(0, 1, 2_000, dest(9), 5_000);
    if let Event::RefundRequested { ask, .. } = &mut by_value {
        *ask = Ask::Value {
            minor: 200,
            minor_per_btc: 10_000_000,
            source: RateSource::new("test-fixed", T0).unwrap(),
        };
    }
    s.push(&[opened(0), finished(0, 5_000), by_value]);
    assert_eq!(s.apply().queued, 1);
    let value = s.owner.refunds()[0].shop_value.unwrap();
    assert_eq!((value.minor, value.currency), (200, *b"USD"));
}

#[test]
fn cr_60_replaying_the_same_chain_adds_nothing() {
    let mut s = Side::new();
    s.push(&[
        opened(0),
        finished(0, 5_000),
        requested(0, 1, 2_000, dest(9), 5_000),
    ]);
    assert_eq!(s.apply().queued, 1);
    // The same copy again, and again after the intake and the copy are read
    // back from disk as a restarted app reads them.
    assert_eq!(s.apply(), crate::Applied::default());
    s.restart_intake();
    assert_eq!(s.apply(), crate::Applied::default());
    let (copy, _) = CounterCopy::open(s.dir.path(), counter(), &devices()).unwrap();
    let evidence = Evidence {
        view: &s.view,
        tip: TIP,
        refund_depth: DEPTH,
    };
    let again = s
        .intake
        .apply(&mut s.owner, counter(), &copy, &evidence, &mut ok_persist());
    assert_eq!(again, crate::Applied::default());
    // The same records offered to the copy again are skipped, so the copy
    // does not grow and nothing is applied.
    let all = s.chain.records().to_vec();
    let taken = s.copy.take(all, &devices());
    assert_eq!((taken.appended, taken.skipped, taken.stopped), (0, 3, None));
    assert_eq!(s.apply(), crate::Applied::default());
    assert_eq!(s.owner.refunds().len(), 1);
    // Later records are applied from where the position stands.
    s.push(&[requested(0, 2, 1_000, dest(8), 5_000)]);
    assert_eq!(s.apply().queued, 1);
    assert_eq!(s.owner.refunds().len(), 2);
}

#[test]
fn cr_60_a_record_with_a_wrong_signature_never_reaches_the_queue() {
    let mut s = Side::new();
    s.push(&[opened(0), finished(0, 5_000)]);
    // A request signed by the counter, with its body changed afterwards.
    let mut forged = Chain::from_records(s.chain.records().to_vec());
    let e = requested(0, 1, 2_000, dest(9), 5_000);
    forged.append(&id(3), e.kind(), e.encode()).unwrap();
    let mut bad = forged.records().last().unwrap().clone();
    bad.body = requested(0, 1, 4_000, dest(7), 5_000).encode();
    let taken = s.copy.take(vec![bad], &devices());
    assert_eq!(taken.appended, 0);
    assert_eq!(taken.stopped, Some(Stop::BadSignature { seq: 2 }));
    // A request signed by a key that is not on the list.
    let mut foreign = Chain::from_records(s.chain.records().to_vec());
    foreign.append(&id(7), e.kind(), e.encode()).unwrap();
    let stranger = foreign.records().last().unwrap().clone();
    let taken = s.copy.take(vec![stranger], &devices());
    assert_eq!(taken.stopped, Some(Stop::UnknownAuthor { seq: 2 }));
    // Neither is in the copy, so the intake has nothing to apply.
    assert_eq!(s.copy.len(), 2);
    assert_eq!(s.apply(), crate::Applied::default());
    assert!(s.owner.refunds().is_empty());
}

#[test]
fn cr_60_a_request_written_by_another_device_on_the_list_is_not_the_counters() {
    let mut s = Side::new();
    s.push(&[opened(0), finished(0, 5_000)]);
    // The owner's own key is on the list, and its record is in the chain the
    // copy holds; it is not a request of the counter.
    let e = requested(0, 1, 2_000, dest(9), 5_000);
    s.chain.append(&id(1), e.kind(), e.encode()).unwrap();
    let taken = s
        .copy
        .take(vec![s.chain.records().last().unwrap().clone()], &devices());
    assert_eq!((taken.appended, taken.stopped), (1, None));
    let done = s.apply();
    assert_eq!(done, crate::Applied::default());
    assert!(s.owner.refunds().is_empty());
    assert_eq!(s.intake.applied_up_to(&counter()), 3, "it was passed over");
}

#[test]
fn cr_66_a_request_below_the_refund_depth_waits_and_follows_the_owners_view() {
    let mut s = Side::with_view(view_with(&[(0, 5_000, 1)]));
    s.push(&[
        opened(0),
        confirmed(0, 5_000),
        requested(0, 1, 2_000, dest(9), 5_000),
    ]);
    assert_eq!(s.apply().queued, 1);
    let id = s.owner.refunds()[0].id;
    assert_eq!(s.owner.waiting_for_depth(id), Some(true));
    assert!(!s.owner.sale_facts(&key(0)).unwrap().reached_refund_depth);
    // The payment reaches the refund depth in the owner's view: the facts
    // follow it and the queued refund stops waiting, with no new request.
    s.view = view_with(&[(0, 5_000, DEPTH)]);
    let done = s.apply();
    assert_eq!((done.queued, done.refused, done.stopped), (0, 0, None));
    assert_eq!(s.owner.waiting_for_depth(id), Some(false));
    // A reorganization that undoes the depth puts it back to waiting.
    s.view = view_with(&[(0, 5_000, 1)]);
    s.apply();
    assert_eq!(s.owner.waiting_for_depth(id), Some(true));
    // What the counter's records say does not matter: a final record from
    // the counter changes nothing.
    s.push(&[finished(0, 5_000)]);
    s.apply();
    assert_eq!(s.owner.waiting_for_depth(id), Some(true));
}

#[test]
fn oc_15_a_sale_the_view_shows_nothing_paid_for_is_refused_and_the_claim_is_noted() {
    let mut s = Side::with_view(view_with(&[]));
    s.push(&[opened(0), requested(0, 1, 2_000, dest(9), 5_000)]);
    let done = s.apply();
    assert_eq!((done.queued, done.refused, done.waiting), (0, 1, None));
    assert!(s.owner.refunds().is_empty());
    assert_eq!(
        s.owner.sale_facts(&key(0)),
        Some(SaleFacts {
            received_sats: 0,
            reached_refund_depth: false
        })
    );
    let notes = s.intake.notes();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].kind, NoteKind::ClaimedMoreThanSeen);
    assert!(notes[0].text.contains("5000") && notes[0].text.contains(" 0 sat"));
    assert_eq!(notes[1].kind, NoteKind::Refused);
    assert!(
        notes[1].text.starts_with("refused: AS-52"),
        "{}",
        notes[1].text
    );
}

#[test]
fn oc_15_a_counter_that_claims_50000_when_20000_arrived_gets_the_20000() {
    let mut s = Side::with_view(view_with(&[(0, 20_000, DEPTH)]));
    s.push(&[
        opened(0),
        finished(0, 50_000),
        requested(0, 1, 30_000, dest(9), 50_000),
    ]);
    let done = s.apply();
    assert_eq!((done.queued, done.refused), (0, 1));
    // The facts are the view's, not the counter's.
    assert_eq!(
        s.owner.sale_facts(&key(0)),
        Some(SaleFacts {
            received_sats: 20_000,
            reached_refund_depth: true
        })
    );
    assert!(s.owner.refunds().is_empty());
    let notes = s.intake.notes();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].kind, NoteKind::ClaimedMoreThanSeen);
    assert!(
        notes[0].text.contains("50000") && notes[0].text.contains("20000"),
        "{}",
        notes[0].text
    );
    // The owner's own rule refused the refund of 30,000.
    assert_eq!(notes[1].kind, NoteKind::Refused);
    assert!(
        notes[1].text.starts_with("refused: AS-52"),
        "{}",
        notes[1].text
    );
    assert!(notes[1].text.contains("20000"), "{}", notes[1].text);
    // A refund within what arrived is queued, with the facts of 20,000.
    s.push(&[requested(0, 2, 20_000, dest(8), 50_000)]);
    let done = s.apply();
    assert_eq!((done.queued, done.refused), (1, 0));
    assert_eq!(
        s.owner.sale_facts(&key(0)),
        Some(SaleFacts {
            received_sats: 20_000,
            reached_refund_depth: true
        })
    );
    // The line is noted for this request too, and the list reads back.
    assert_eq!(s.intake.notes().len(), 3);
    s.restart_intake();
    assert_eq!(s.intake.notes()[0].kind, NoteKind::ClaimedMoreThanSeen);
}

#[test]
fn oc_15_a_claim_below_what_the_view_shows_is_not_noted() {
    // A late payment or a surplus: the view shows more than the counter says.
    let mut s = Side::with_view(view_with(&[(0, 5_000, DEPTH), (0, 1_000, DEPTH)]));
    s.push(&[
        opened(0),
        finished(0, 5_000),
        requested(0, 1, 4_500, dest(9), 5_000),
    ]);
    let done = s.apply();
    assert_eq!(
        (done.queued, done.refused),
        (1, 0),
        "{:?}",
        s.intake.notes()
    );
    assert!(s.intake.notes().is_empty());
    assert_eq!(s.owner.sale_facts(&key(0)).unwrap().received_sats, 6_000);
}

#[test]
fn oc_15_a_request_waits_for_the_view_and_is_applied_on_a_later_run() {
    // A view that has checked nothing yet.
    let mut s = Side::with_view(CoinView::new(0));
    s.push(&[
        opened(0),
        finished(0, 5_000),
        requested(0, 1, 2_000, dest(9), 5_000),
        withdrawn(0, 1),
        requested(0, 2, 1_000, dest(8), 5_000),
    ]);
    let done = s.apply();
    assert_eq!(done.waiting, Some(FactsRefused::ViewNotReady));
    assert_eq!((done.queued, done.refused, done.stopped), (0, 0, None));
    assert!(s.owner.refunds().is_empty());
    assert!(s.intake.notes().is_empty(), "not listed as refused");
    assert_eq!(s.intake.applied_up_to(&counter()), 2, "stays before it");
    // And again, and after a restart: still not dropped, no time limit.
    assert_eq!(s.apply().waiting, Some(FactsRefused::ViewNotReady));
    s.restart_intake();
    assert_eq!(s.apply().waiting, Some(FactsRefused::ViewNotReady));
    assert!(s.intake.notes().is_empty());
    // A view that is behind the tip waits too.
    s.view = view_with(&[(0, 5_000, DEPTH)]);
    s.view.reset(|_| Ok::<(), ()>(())).unwrap();
    assert_eq!(s.apply().waiting, Some(FactsRefused::ViewNotReady));
    // The view catches up: the requests are applied in order, the withdrawal
    // after the first one removes it again.
    s.view = view_with(&[(0, 5_000, DEPTH)]);
    let done = s.apply();
    assert_eq!(done.waiting, None);
    assert_eq!((done.queued, done.withdrawn, done.refused), (2, 1, 0));
    assert_eq!(s.owner.refunds().len(), 1);
    assert_eq!(s.owner.refunds()[0].sats, 1_000);
    assert!(s.intake.notes().is_empty());
}

#[test]
fn oc_14_an_index_never_issued_to_the_counter_is_refused_and_listed() {
    // Outside every range the owner issued.
    let mut s = Side::with_view(view_with(&[(25, 5_000, DEPTH)]));
    s.push(&[
        opened(25),
        finished(25, 5_000),
        requested(25, 1, 2_000, dest(9), 5_000),
    ]);
    let done = s.apply();
    assert_eq!((done.queued, done.refused, done.waiting), (0, 1, None));
    assert!(s.owner.refunds().is_empty());
    let notes = s.intake.notes();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].kind, NoteKind::Refused);
    assert!(
        notes[0].text.starts_with("refused: OC-14"),
        "{}",
        notes[0].text
    );
    assert_eq!((notes[0].sale, notes[0].number), (25, 1));
    // Not retried.
    assert_eq!(s.apply(), crate::Applied::default());

    // An index inside a range that was issued to another counter.
    let mut s = Side::new();
    s.owner = new_owner_for(id(4).device_id());
    s.push(&[
        opened(0),
        finished(0, 5_000),
        requested(0, 1, 2_000, dest(9), 5_000),
    ]);
    let done = s.apply();
    assert_eq!((done.queued, done.refused), (0, 1));
    assert!(s.owner.refunds().is_empty());
    assert!(s.intake.notes()[0].text.starts_with("refused: OC-14"));
}

#[test]
fn oc_14_two_payments_to_one_index_are_deep_only_when_both_are() {
    let mut s = Side::with_view(view_with(&[(0, 3_000, DEPTH), (0, 2_000, 2)]));
    s.push(&[opened(0), requested(0, 1, 4_000, dest(9), 5_000)]);
    assert_eq!(s.apply().queued, 1);
    let id = s.owner.refunds()[0].id;
    assert_eq!(
        s.owner.sale_facts(&key(0)),
        Some(SaleFacts {
            received_sats: 5_000,
            reached_refund_depth: false
        })
    );
    assert_eq!(s.owner.waiting_for_depth(id), Some(true));
    // The shallow payment gets one block short of the depth: still waiting.
    s.view = view_with(&[(0, 3_000, DEPTH), (0, 2_000, DEPTH - 1)]);
    s.apply();
    assert_eq!(s.owner.waiting_for_depth(id), Some(true));
    // Both deep.
    s.view = view_with(&[(0, 3_000, DEPTH + 4), (0, 2_000, DEPTH)]);
    s.apply();
    assert_eq!(s.owner.waiting_for_depth(id), Some(false));
}

#[test]
fn cr_70_a_request_withdrawn_before_a_session_is_removed_from_the_queue() {
    let mut s = Side::new();
    s.push(&[
        opened(0),
        finished(0, 5_000),
        requested(0, 1, 2_000, dest(9), 5_000),
        requested(0, 2, 1_000, dest(8), 5_000),
    ]);
    assert_eq!(s.apply().queued, 2);
    s.push(&[withdrawn(0, 1)]);
    let done = s.apply();
    assert_eq!((done.withdrawn, done.held, done.stopped), (1, 0, None));
    let refunds = s.owner.refunds();
    assert_eq!(refunds.len(), 1);
    assert_eq!(refunds[0].sats, 1_000);
    assert_eq!(s.intake.queued_id(&counter(), 1), None);
    assert!(s.intake.queued_id(&counter(), 2).is_some());
    // The amount is free again in the owner's count (AS-52).
    assert_eq!(s.owner.refunded_sats(&key(0)), 1_000);
    // Withdrawn twice, or never queued: nothing happens.
    s.push(&[withdrawn(0, 1), withdrawn(0, 77)]);
    assert_eq!(s.apply().withdrawn, 0);
    assert_eq!(s.owner.refunds().len(), 1);
}

#[test]
fn cr_70_a_request_withdrawn_after_a_session_opened_stays_and_a_note_says_so() {
    let mut s = Side::new();
    s.push(&[
        opened(0),
        finished(0, 5_000),
        requested(0, 1, 2_000, dest(9), 5_000),
    ]);
    assert_eq!(s.apply().queued, 1);
    // The owner opens an approval session: the refund is frozen into it.
    s.owner
        .build_request(
            &[coin(1, 0, 300_000)],
            1_000,
            0,
            0,
            10_000_000,
            &[7u8; 96],
            |_| Ok::<(), String>(()),
        )
        .unwrap();
    assert!(matches!(
        s.owner.refunds()[0].stage,
        Stage::InSession { .. }
    ));
    s.push(&[withdrawn(0, 1)]);
    let done = s.apply();
    assert_eq!((done.withdrawn, done.held, done.stopped), (0, 1, None));
    assert_eq!(s.owner.refunds().len(), 1, "the refund stays");
    let notes = s.intake.notes();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].kind, NoteKind::WithdrawalNotApplied);
    assert!(notes[0].text.contains("open session"), "{}", notes[0].text);
    assert!(notes[0].withdrawn);
    // It is not tried again.
    assert_eq!(s.apply(), crate::Applied::default());
    assert_eq!(s.intake.notes().len(), 1);
}

#[test]
fn cr_70_a_withdrawal_of_a_request_that_was_refused_marks_its_line() {
    let mut s = Side::new();
    s.push(&[
        opened(0),
        finished(0, 5_000),
        requested(0, 1, 3_000, dest(8), 5_000),
        requested(0, 2, 3_000, dest(9), 5_000),
    ]);
    assert_eq!(s.apply().refused, 1);
    s.push(&[withdrawn(0, 2)]);
    let done = s.apply();
    assert_eq!(done.withdrawn + done.held, 0);
    let notes = s.intake.notes();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].withdrawn);
}

#[test]
fn cr_60_a_request_the_owners_queue_refuses_is_listed_with_its_rule_and_not_retried() {
    let mut s = Side::new();
    // 3,000 sats twice from a sale that received 5,000; and 100 sats, below
    // the dust limit of the destination.
    s.push(&[
        opened(0),
        finished(0, 5_000),
        requested(0, 1, 3_000, dest(7), 5_000),
        requested(0, 2, 3_000, dest(9), 5_000),
        requested(0, 3, 100, dest(8), 5_000),
    ]);
    let done = s.apply();
    assert_eq!((done.queued, done.refused), (1, 2));
    assert_eq!(s.owner.refunds().len(), 1);
    assert_eq!(s.owner.refunds()[0].sats, 3_000);
    let notes = s.intake.notes();
    assert_eq!(notes.len(), 2);
    assert!(
        notes[0].text.starts_with("refused: AS-52"),
        "{}",
        notes[0].text
    );
    assert_eq!(
        (notes[0].number, notes[0].sale, notes[0].kind),
        (2, 0, NoteKind::Refused)
    );
    assert!(
        notes[1].text.starts_with("refused: AS-51"),
        "{}",
        notes[1].text
    );
    assert_eq!(notes[1].number, 3);
    // Never retried: the same chain again, from a restarted intake, adds
    // nothing, and the list is read back as it was.
    assert_eq!(s.apply(), crate::Applied::default());
    s.restart_intake();
    assert_eq!(s.apply(), crate::Applied::default());
    assert_eq!(s.intake.notes().len(), 2);
    assert_eq!(s.owner.refunds().len(), 1);
    // The owner dismisses a line.
    s.intake.dismiss(0).unwrap();
    assert_eq!(s.intake.notes().len(), 1);
    s.restart_intake();
    assert_eq!(s.intake.notes().len(), 1);
    assert_eq!(s.intake.notes()[0].number, 3);
}

#[test]
fn cr_60_a_refund_record_that_does_not_read_is_listed() {
    let mut s = Side::new();
    s.push(&[opened(0)]);
    s.chain
        .append(&id(3), RecordKind::Refund, vec![99, 1, 2, 3])
        .unwrap();
    let new = vec![s.chain.records().last().unwrap().clone()];
    assert_eq!(s.copy.take(new, &devices()).stopped, None);
    let done = s.apply();
    assert_eq!(done.refused, 1);
    assert!(s.intake.notes()[0].text.starts_with("refused:"));
    assert_eq!(s.apply(), crate::Applied::default());
}

#[test]
fn cr_60_an_owner_store_that_refuses_the_queue_is_tried_again_on_the_next_call() {
    let mut s = Side::new();
    s.push(&[
        opened(0),
        finished(0, 5_000),
        requested(0, 1, 2_000, dest(9), 5_000),
    ]);
    let mut failing: Persist = Box::new(|_| Err("disk full".to_string()));
    let done = s.apply_with(&mut failing);
    assert_eq!(done.queued, 0);
    assert!(matches!(done.stopped, Some(Stopped::Owner(_))));
    assert!(s.owner.refunds().is_empty());
    assert!(s.intake.notes().is_empty(), "not a refusal");
    // The position is back before the request.
    assert_eq!(s.intake.applied_up_to(&counter()), 2);
    let done = s.apply();
    assert_eq!((done.queued, done.stopped), (1, None));
    assert_eq!(s.owner.refunds().len(), 1);
    assert_eq!(s.apply(), crate::Applied::default());
}

#[test]
fn cr_60_a_request_stopped_before_its_result_was_stored_is_listed_at_the_next_start() {
    let mut s = Side::new();
    // The state a stop in the middle of applying request 4 leaves: the
    // position moved past it and the mark of it is in the file.
    let mut file = Vec::from(MAGIC_INTAKE);
    file.push(1);
    file.extend(1u32.to_le_bytes());
    file.extend(counter().to_bytes());
    file.extend(3u64.to_le_bytes()); // applied up to
    file.push(1); // pending
    file.extend(counter().to_bytes());
    file.extend(2u64.to_le_bytes()); // seq
    file.extend(4u64.to_le_bytes()); // number
    file.extend(0u32.to_le_bytes()); // sale
    file.extend(0u32.to_le_bytes()); // queued
    file.extend(0u32.to_le_bytes()); // notes
    fs::write(s.dir.path().join(STATE_FILE), &file).unwrap();
    s.restart_intake();
    let notes = s.intake.notes();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].kind, NoteKind::Interrupted);
    assert_eq!((notes[0].seq, notes[0].number), (2, 4));
    assert!(notes[0].text.contains("check the queue"));
    // The mark is cleared, and the line is kept across the next start.
    s.restart_intake();
    assert_eq!(s.intake.notes().len(), 1);
    assert_eq!(s.intake.applied_up_to(&counter()), 3);
}

#[test]
fn cr_60_the_intake_state_is_strict() {
    let mut s = Side::new();
    s.push(&[
        opened(0),
        finished(0, 5_000),
        requested(0, 1, 3_000, dest(7), 5_000),
        requested(0, 2, 3_000, dest(9), 5_000),
        requested(0, 3, 1_000, dest(8), 5_000),
    ]);
    s.apply();
    let path = s.dir.path().join(STATE_FILE);
    let good = fs::read(&path).unwrap();
    assert_eq!(&good[..5], b"OBM1\x01");
    for n in 0..good.len() {
        fs::write(&path, &good[..n]).unwrap();
        assert!(RefundIntake::open(s.dir.path()).is_err(), "prefix {n}");
    }
    let mut longer = good.clone();
    longer.push(0);
    fs::write(&path, &longer).unwrap();
    assert!(RefundIntake::open(s.dir.path()).is_err());
    let mut other = good.clone();
    other[3] = b'X';
    fs::write(&path, &other).unwrap();
    assert!(RefundIntake::open(s.dir.path()).is_err());
    fs::write(&path, &good).unwrap();
    let again = RefundIntake::open(s.dir.path()).unwrap();
    assert_eq!(again.notes(), s.intake.notes());
    assert_eq!(
        again.queued_id(&counter(), 3),
        s.intake.queued_id(&counter(), 3)
    );
}

#[test]
fn cr_71_a_cash_refund_asks_the_owner_for_nothing_and_is_listed() {
    let mut s = Side::new();
    s.push(&[opened(0), finished(0, 5_000), paid_in_cash(0, 25)]);
    let done = s.apply();
    assert_eq!(done, crate::Applied::default());
    assert!(s.owner.refunds().is_empty());
    let seen = RefundIntake::cash_refunds(&s.copy);
    assert_eq!(seen.len(), 1);
    assert_eq!(
        (seen[0].sale, seen[0].minor, seen[0].name.as_str()),
        (0, 25, "ann")
    );
    assert_eq!(s.intake.applied_up_to(&counter()), 3, "it was passed over");
}

#[test]
fn cr_60_the_list_is_bounded_and_says_what_went_off_it() {
    let mut s = Side::new();
    s.push(&[opened(0), finished(0, 5_000)]);
    // More refused requests than the list holds.
    let over: Vec<Event> = (1..=(MAX_NOTES as u64 + 3))
        .map(|n| requested(0, n, 100, dest(9), 5_000))
        .collect();
    s.push(&over);
    let done = s.apply();
    assert_eq!(done.refused, MAX_NOTES as u64 + 3);
    assert_eq!(s.intake.notes().len(), MAX_NOTES);
    assert_eq!(s.intake.dropped(), 3);
    assert_eq!(s.intake.notes()[0].number, 4);
}

// ---- the whole path through the link ---------------------------------------------

mod through_the_link {
    use openbmt_core::records::Record;
    use openbmt_courier::{records_content, Courier, FirstStart, MemoryPost, MemoryRelay};
    use openbmt_link::{Kind, Role as LinkRole};

    use super::*;
    use crate::{OwnerLink, OwnerLinkConfig};

    #[test]
    fn a_request_sent_through_the_relay_is_queued_once_and_a_withdrawal_follows() {
        let relay = MemoryRelay::new(id(99), &[id(1).device_id(), counter()]).unwrap();
        let (owner_dir, counter_dir, watcher_dir) = (Dir::new(), Dir::new(), Dir::new());
        let mut rig = start(watcher_dir.path());
        let owner_courier = Courier::start_with(
            relay.post(),
            id(1),
            LinkRole::Owner,
            [(counter(), LinkRole::Counter)].into_iter().collect(),
            owner_dir.path(),
            FirstStart::Yes,
        )
        .unwrap();
        let mut link = OwnerLink::new(
            owner_courier,
            owner_dir.path(),
            devices(),
            vec![counter()],
            OwnerLinkConfig {
                poll_secs: 0,
                retry_secs: 0,
                ..OwnerLinkConfig::default()
            },
        )
        .unwrap();
        let mut counter_side: Courier<MemoryPost> = Courier::start_with(
            relay.post(),
            id(3),
            LinkRole::Counter,
            [(id(1).device_id(), LinkRole::Owner)].into_iter().collect(),
            counter_dir.path(),
            FirstStart::Yes,
        )
        .unwrap();
        counter_side.poll().unwrap();
        link.courier_mut().poll().unwrap();

        let mut intake = RefundIntake::open(owner_dir.path()).unwrap();
        let mut owner = new_owner();
        let view = view_with(&[(0, 5_000, DEPTH)]);
        let evidence = Evidence {
            view: &view,
            tip: TIP,
            refund_depth: DEPTH,
        };
        let mut persist = ok_persist();

        let mut chain = Chain::new();
        let send = |chain: &mut Chain, side: &mut Courier<MemoryPost>, events: &[Event]| {
            let before = chain.records().len();
            for e in events {
                chain.append(&id(3), e.kind(), e.encode()).unwrap();
            }
            let new: Vec<Record> = chain.records()[before..].to_vec();
            let (content, count) = records_content(&new, usize::MAX);
            assert_eq!(count, new.len());
            side.send(&id(1).device_id(), Kind::Records, &content)
                .unwrap();
        };
        send(
            &mut chain,
            &mut counter_side,
            &[
                opened(0),
                finished(0, 5_000),
                requested(0, 1, 2_000, dest(9), 5_000),
            ],
        );
        link.step(&mut rig.watcher, T0);
        let done = intake.run(&mut owner, &link, &evidence, &mut persist);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].0, counter());
        assert_eq!(done[0].1.queued, 1);
        assert_eq!(owner.refunds().len(), 1);
        assert_eq!(owner.refunds()[0].script, dest(9));
        // Nothing new: nothing applied.
        link.step(&mut rig.watcher, T0 + 1);
        assert!(intake
            .run(&mut owner, &link, &evidence, &mut persist)
            .is_empty());
        // The withdrawal comes the same way.
        send(&mut chain, &mut counter_side, &[withdrawn(0, 1)]);
        link.step(&mut rig.watcher, T0 + 2);
        let done = intake.run(&mut owner, &link, &evidence, &mut persist);
        assert_eq!(done[0].1.withdrawn, 1);
        assert!(owner.refunds().is_empty());
        // A copy that grew with something else costs nothing.
        send(&mut chain, &mut counter_side, &[confirmed(0, 5_000)]);
        link.step(&mut rig.watcher, T0 + 3);
        assert!(intake
            .run(&mut owner, &link, &evidence, &mut persist)
            .is_empty());
    }
}

// ---- the counter's lists agree with the signer's and the owner's -------------------

/// Scripts of every shape the lists could disagree about.
fn scripts() -> Vec<ScriptBuf> {
    use bitcoin::key::{PublicKey, XOnlyPublicKey};
    use bitcoin::secp256k1::SecretKey;
    use bitcoin::{PubkeyHash, ScriptHash, WScriptHash, WitnessProgram, WitnessVersion};
    let sk = SecretKey::from_slice(&[7; 32]).unwrap();
    let pk = PublicKey::new(sk.public_key(secp()));
    let (xonly, _) = XOnlyPublicKey::from_keypair(&sk.keypair(secp()));
    let mut list = vec![
        ScriptBuf::new_p2pkh(&PubkeyHash::from_byte_array([1; 20])),
        ScriptBuf::new_p2sh(&ScriptHash::from_byte_array([2; 20])),
        ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([3; 20])),
        ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([4; 32])),
        ScriptBuf::new_p2tr(secp(), xonly, None),
        ScriptBuf::new_p2pk(&pk),
        ScriptBuf::new_op_return([1, 2, 3]),
        ScriptBuf::new(),
        ScriptBuf::new_witness_program(
            &WitnessProgram::new(WitnessVersion::V2, &[5u8; 32]).unwrap(),
        ),
        ScriptBuf::new_witness_program(
            &WitnessProgram::new(WitnessVersion::V1, &[0x4e, 0x73]).unwrap(),
        ),
        ScriptBuf::from_bytes(vec![0x51]),
        ScriptBuf::from_bytes(vec![0x00, 0x14, 1, 2, 3]),
        ScriptBuf::from_bytes(vec![
            0x51, 0x21, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
            9, 9, 9, 9, 9, 9, 9, 9,
        ]),
    ];
    // A bare multisig.
    let mut multisig = vec![0x51, 0x21];
    multisig.extend(pk.to_bytes());
    multisig.extend([0x51, 0xae]);
    list.push(ScriptBuf::from_bytes(multisig));
    list
}

#[test]
fn cr_63_the_counters_list_of_payable_scripts_is_the_signers_and_the_owners() {
    let k = kit();
    let signer = openbmt_signer::SignerState::register(
        k.wallet.canonical_string(),
        NET,
        &k.xpubs[0],
        Some(&k.xpubs[2]),
    )
    .unwrap();
    let mut payable = 0;
    for script in scripts() {
        let counter_says = openbmt_counter::refund::is_payment_script(&script);
        let owner_says = openbmt_spend::dust_limit(&script).is_some();
        let mut copy = signer.clone();
        let signer_says = copy
            .enroll_destination(script.clone(), "shop", 1_000, |_| Ok::<(), String>(()))
            .is_ok();
        assert_eq!(counter_says, owner_says, "{script:?}");
        assert_eq!(counter_says, signer_says, "{script:?}");
        assert_eq!(
            openbmt_counter::refund::dust_limit(&script),
            openbmt_spend::dust_limit(&script),
            "{script:?}"
        );
        payable += usize::from(counter_says);
    }
    assert_eq!(payable, 5, "the five kinds, and no other");
}

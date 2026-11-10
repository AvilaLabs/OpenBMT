//! A sale watch saved and restored, and the amount it is judged against.

use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, ScriptBuf, Transaction};
use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::{
    PeerAddr, SaleStatus, SaleWatch, Source, MAX_ENTRY_BLOCKS, MAX_ENTRY_GROUPS,
    MAX_SALE_TRANSACTIONS, MAX_TX_BYTES,
};

const NOW: u64 = 1_000_000;

fn shop() -> ScriptBuf {
    script(1)
}

fn peer(n: u8) -> Source {
    Source::Peer(PeerAddr::Ip(format!("{n}.{n}.1.1").parse().unwrap()))
}

fn pays(prev: u8, sats: u64) -> Transaction {
    spend(prev, &[(&shop(), sats)])
}

fn sight(w: &mut SaleWatch, tx: &Transaction, source: Source, now: u64) {
    w.sighting(&bytes(tx), source, now).unwrap();
}

fn seen_by(w: &mut SaleWatch, tx: &Transaction, groups: u8, now: u64) {
    for n in 20..20 + groups {
        sight(w, tx, peer(n), now);
    }
}

fn confirm_in(w: &mut SaleWatch, chain: &mut TestChain, txs: &[Transaction], height: u32) {
    let mut all = vec![coinbase(height as u8)];
    all.extend(txs.iter().cloned());
    let b = block_at(all, height);
    chain.add(b.block_hash(), height);
    for position in 1..b.txdata.len() {
        w.confirmed(proven(&b, position as u32, height)).unwrap();
    }
}

/// Save, restore, and check the bytes are the same again and the restored
/// sale answers like the original at `now`.
fn round_trip(w: &SaleWatch, chain: &TestChain, now: u64) -> SaleWatch {
    let saved = w.to_bytes();
    let back = SaleWatch::from_bytes(&saved).unwrap();
    assert_eq!(back.to_bytes(), saved);
    assert_eq!(status_of(w, chain, now), status_of(&back, chain, now));
    assert_eq!(w.script(), back.script());
    assert_eq!(w.sale_sats(), back.sale_sats());
    assert_eq!(w.watched_outpoints(), back.watched_outpoints());
    assert!(w.transactions().eq(back.transactions()));
    back
}

/// The status, on a copy so that the memory of the last status is untouched.
fn status_of(w: &SaleWatch, chain: &TestChain, now: u64) -> SaleStatus {
    w.clone().status(chain, now)
}

#[test]
fn set_sale_sats_moves_only_the_amount() {
    let mut chain = TestChain::new(100);
    let mut w = SaleWatch::new(shop(), 10_000, 0);
    confirm_in(&mut w, &mut chain, &[pays(1, 8_000)], 100);
    assert_eq!(w.sale_sats(), 10_000);
    // Short: not confirmed.
    assert_eq!(status_at(&mut w, &chain, NOW).min_confirmations, 0);
    let before = w.to_bytes();
    // Lowered: enough.
    w.set_sale_sats(8_000);
    assert_eq!(w.sale_sats(), 8_000);
    let s = status_at(&mut w, &chain, NOW);
    assert_eq!((s.received_sats, s.min_confirmations), (8_000, 1));
    // Raised again: short again. The evidence never changed.
    w.set_sale_sats(8_001);
    assert_eq!(status_at(&mut w, &chain, NOW).min_confirmations, 0);
    assert_eq!(w.transactions().count(), 1);
    w.set_sale_sats(10_000);
    assert_eq!(w.to_bytes(), before);
    assert_eq!(w.script(), shop().as_script());
}

#[test]
fn an_empty_sale_round_trips() {
    let w = SaleWatch::new(shop(), 12_345, 250);
    let chain = TestChain::new(10);
    let back = round_trip(&w, &chain, NOW);
    assert_eq!(back.sale_sats(), 12_345);
    assert_eq!(back.transactions().count(), 0);
    // magic and version
    assert_eq!(&w.to_bytes()[..5], b"OBMV\x01");
}

#[test]
fn a_seen_payment_round_trips() {
    let mut w = SaleWatch::new(shop(), 10_000, 100);
    let tx = pays(1, 10_000);
    seen_by(&mut w, &tx, 3, NOW);
    sight(&mut w, &tx, Source::Box, NOW + 2);
    let chain = TestChain::new(10);
    let back = round_trip(&w, &chain, NOW + 20);
    let s = status_of(&back, &chain, NOW + 20);
    assert_eq!(s.peer_groups, 3);
    assert_eq!(s.secs_since_first_seen, 20);
    assert!(s.mode.is_some());
}

#[test]
fn a_fee_bump_round_trips_and_the_replaced_transaction_stays_ignored() {
    let mut w = SaleWatch::new(shop(), 10_000, 0);
    let chain = TestChain::new(10);
    let old = spend(1, &[(&shop(), 10_000)]);
    let bump = spend(1, &[(&shop(), 10_000), (&script(0x55), 5)]);
    seen_by(&mut w, &old, 3, NOW);
    seen_by(&mut w, &bump, 2, NOW + 1);
    let mut back = round_trip(&w, &chain, NOW + 5);
    assert_eq!(back.transactions().count(), 1);
    // A slow peer still relays the old one: not a conflict, before or after.
    sight(&mut w, &old, peer(40), NOW + 6);
    sight(&mut back, &old, peer(40), NOW + 6);
    assert!(!status_of(&back, &chain, NOW + 7).conflict_seen);
    assert_eq!(back.to_bytes(), w.to_bytes());
}

#[test]
fn a_conflict_round_trips() {
    let mut w = SaleWatch::new(shop(), 10_000, 0);
    let chain = TestChain::new(10);
    seen_by(&mut w, &pays(1, 10_000), 3, NOW);
    sight(&mut w, &pays(1, 3_000), peer(30), NOW);
    let back = round_trip(&w, &chain, NOW);
    assert!(status_of(&back, &chain, NOW).conflict_seen);
    assert_eq!(back.transactions().count(), 2);
}

#[test]
fn a_confirmation_and_its_reorg_round_trip() {
    let mut chain = TestChain::new(100);
    let mut w = SaleWatch::new(shop(), 10_000, 0);
    let tx = pays(1, 10_000);
    seen_by(&mut w, &tx, 3, NOW);
    confirm_in(&mut w, &mut chain, std::slice::from_ref(&tx), 100);
    let hash = {
        let b = block_at(vec![coinbase(100), tx], 100);
        b.block_hash()
    };
    // The memory of the last status is saved too.
    assert_eq!(status_at(&mut w, &chain, NOW).min_confirmations, 1);
    let confirmed = round_trip(&w, &chain, NOW);
    assert_eq!(status_of(&confirmed, &chain, NOW).min_confirmations, 1);
    chain.remove(&hash);
    let mut back = round_trip(&w, &chain, NOW);
    let mut original = w.clone();
    let (a, b) = (original.status(&chain, NOW), back.status(&chain, NOW));
    assert!(a.unconfirmed_again);
    assert_eq!(a, b);
}

#[test]
fn a_spend_elsewhere_round_trips() {
    let mut chain = TestChain::new(100);
    let mut w = SaleWatch::new(shop(), 10_000, 0);
    seen_by(&mut w, &pays(1, 10_000), 3, NOW);
    let other = spend(1, &[(&script(0x55), 10_000)]);
    let b = block(vec![coinbase(9), other.clone()]);
    chain.add(b.block_hash(), 100);
    w.spent_elsewhere(coin(1), other.compute_txid(), b.block_hash());
    assert_eq!(status_at(&mut w, &chain, NOW).received_sats, 0);
    let back = round_trip(&w, &chain, NOW);
    assert_eq!(back.spent_elsewhere_in(), w.spent_elsewhere_in());
    assert!(status_of(&back, &chain, NOW).conflict_seen);
    // The block leaves the chain: the payment stands again, in both.
    chain.remove(&b.block_hash());
    assert_eq!(status_of(&back, &chain, NOW).received_sats, 10_000);
}

#[test]
fn a_sale_that_needs_a_person_round_trips() {
    let mut w = SaleWatch::new(shop(), 1_000_000, 0);
    for n in 0..MAX_SALE_TRANSACTIONS as u8 {
        sight(&mut w, &pays(n + 1, 100), peer(20), NOW);
    }
    assert!(w.sighting(&bytes(&pays(100, 100)), peer(20), NOW).is_err());
    let chain = TestChain::new(1);
    let back = round_trip(&w, &chain, NOW);
    assert!(status_of(&back, &chain, NOW).needs_a_person);
    assert_eq!(back.transactions().count(), MAX_SALE_TRANSACTIONS);
}

#[test]
fn the_latest_time_is_saved() {
    // The latest time is saved: a sighting with an earlier clock afterwards
    // does not move the time back, restored or not.
    let mut w = SaleWatch::new(shop(), 10_000, 0);
    let tx = pays(1, 10_000);
    sight(&mut w, &tx, Source::Box, NOW + 100);
    let mut back = SaleWatch::from_bytes(&w.to_bytes()).unwrap();
    sight(&mut w, &tx, peer(20), NOW);
    sight(&mut back, &tx, peer(20), NOW);
    assert_eq!(back.to_bytes(), w.to_bytes());
}

#[test]
fn more_than_64_groups_and_blocks_are_not_kept() {
    let mut w = SaleWatch::new(shop(), 10_000, 0);
    let tx = pays(1, 10_000);
    for n in 20..20 + (MAX_ENTRY_GROUPS as u8 + 6) {
        sight(&mut w, &tx, peer(n), NOW);
    }
    let chain = TestChain::new(1);
    assert_eq!(status_of(&w, &chain, NOW).peer_groups, MAX_ENTRY_GROUPS);
    let mut chain = TestChain::new(200);
    for h in 1..=(MAX_ENTRY_BLOCKS as u32 + 3) {
        confirm_in(&mut w, &mut chain, std::slice::from_ref(&tx), h);
    }
    round_trip(&w, &chain, NOW);
}

// Refusals. The sale below has one transaction seen by two groups and the
// Box, so every field of the format is there. `L` is the transaction's length.

struct Sample {
    bytes: Vec<u8>,
    tx_len: usize,
}

impl Sample {
    fn new() -> Self {
        let mut w = SaleWatch::new(shop(), 10_000, 100);
        let tx = pays(1, 10_000);
        seen_by(&mut w, &tx, 2, NOW + 5);
        sight(&mut w, &tx, Source::Box, NOW + 5);
        let bytes = w.to_bytes();
        let tx_len = self::bytes(&tx).len();
        let s = Self { bytes, tx_len };
        assert_eq!(s.bytes.len(), 142 + tx_len);
        s
    }

    // Where the fields are: 5 for the frame, 4 + 22 for the script, 8 sale
    // amount, 4 tolerance, 2 flags, 8 latest, 4 for the count of
    // transactions; then the transaction.
    const SCRIPT_LEN: usize = 5;
    const TOLERANCE: usize = 39;
    const CONFLICT: usize = 43;
    const NEEDS: usize = 44;
    const LATEST: usize = 45;
    const COUNT: usize = 53;
    const TXID: usize = 57;
    const TX_LEN: usize = 89;
    const TX: usize = 93;

    fn paid(&self) -> usize {
        Self::TX + self.tx_len
    }
    fn n_groups(&self) -> usize {
        self.paid() + 8
    }
    fn groups(&self) -> usize {
        self.n_groups() + 4
    }
    fn by_box(&self) -> usize {
        self.groups() + 10
    }
    fn first_seen_flag(&self) -> usize {
        self.by_box() + 1
    }
    fn first_seen(&self) -> usize {
        self.first_seen_flag() + 1
    }
    fn n_blocks(&self) -> usize {
        self.first_seen() + 8
    }
    fn learned_flag(&self) -> usize {
        self.n_blocks() + 4
    }
    fn n_confirmed(&self) -> usize {
        self.learned_flag() + 1
    }
    fn n_replaced(&self) -> usize {
        self.n_confirmed() + 4
    }
    fn n_elsewhere(&self) -> usize {
        self.n_replaced() + 4
    }

    fn with(&self, edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut b = self.bytes.clone();
        edit(&mut b);
        b
    }

    /// Put a u32 LE at `at`.
    fn set_u32(&self, at: usize, v: u32) -> Vec<u8> {
        self.with(|b| b[at..at + 4].copy_from_slice(&v.to_le_bytes()))
    }
}

fn non_canonical(bytes: &[u8]) {
    match SaleWatch::from_bytes(bytes) {
        Err(DecodeError::NonCanonical(_)) => {}
        other => panic!(
            "expected NonCanonical, got {:?}",
            other.map(|w| w.to_bytes())
        ),
    }
}

#[test]
fn the_sample_is_laid_out_as_the_offsets_say() {
    let s = Sample::new();
    assert!(SaleWatch::from_bytes(&s.bytes).is_ok());
    assert_eq!(s.bytes[Sample::TXID..Sample::TX_LEN], {
        let tx = pays(1, 10_000);
        tx.compute_txid().to_byte_array()
    });
    assert_eq!(s.bytes[s.by_box()], 1);
    assert_eq!(s.bytes[s.first_seen_flag()], 1);
    assert_eq!(s.bytes[s.n_groups()], 2);
}

#[test]
fn a_wrong_frame_is_refused() {
    let s = Sample::new();
    assert_eq!(
        SaleWatch::from_bytes(&s.with(|b| b[0] = b'Z')).err(),
        Some(DecodeError::BadMagic)
    );
    for v in [0u8, 2] {
        assert_eq!(
            SaleWatch::from_bytes(&s.with(|b| b[4] = v)).err(),
            Some(DecodeError::UnsupportedVersion(v))
        );
    }
    assert_eq!(
        SaleWatch::from_bytes(&s.with(|b| b.push(0))).err(),
        Some(DecodeError::TrailingBytes)
    );
}

#[test]
fn cutting_the_bytes_anywhere_is_refused() {
    let mut chain = TestChain::new(100);
    let mut w = SaleWatch::new(shop(), 10_000, 100);
    let tx = pays(1, 10_000);
    seen_by(&mut w, &tx, 3, NOW);
    confirm_in(&mut w, &mut chain, &[tx], 100);
    status_at(&mut w, &chain, NOW);
    let other = spend(2, &[(&script(0x55), 1)]);
    w.spent_elsewhere(
        coin(1),
        other.compute_txid(),
        BlockHash::from_byte_array([9; 32]),
    );
    for bytes in [w.to_bytes(), Sample::new().bytes] {
        for n in 0..bytes.len() {
            assert!(SaleWatch::from_bytes(&bytes[..n]).is_err(), "{n}");
        }
        assert!(SaleWatch::from_bytes(&bytes).is_ok());
    }
}

#[test]
fn the_transaction_id_and_the_amount_are_worked_out_again() {
    let s = Sample::new();
    non_canonical(&s.with(|b| b[Sample::TXID] ^= 1));
    non_canonical(&s.with(|b| b[s.paid()] ^= 1));
    // A paid amount of zero for a transaction that pays the sale.
    non_canonical(&s.with(|b| b[s.paid()..s.paid() + 8].fill(0)));
    // A transaction that does not parse, and one that has bytes after it.
    non_canonical(&s.with(|b| b[Sample::TX + 3] ^= 0xff));
    let shorter = s.set_u32(Sample::TX_LEN, s.tx_len as u32 - 1);
    assert!(SaleWatch::from_bytes(&shorter).is_err());
}

#[test]
fn a_transaction_stored_twice_is_refused() {
    let s = Sample::new();
    let entry = s.bytes[Sample::TXID..s.n_confirmed()].to_vec();
    let twice = s.with(|b| {
        b[Sample::COUNT..Sample::COUNT + 4].copy_from_slice(&2u32.to_le_bytes());
        let at = s.n_confirmed();
        b.splice(at..at, entry.iter().copied());
    });
    non_canonical(&twice);
}

#[test]
fn the_caps_of_the_live_code_hold() {
    let s = Sample::new();
    let too_large = |bytes: Vec<u8>| {
        assert_eq!(
            SaleWatch::from_bytes(&bytes).err(),
            Some(DecodeError::TooLarge)
        );
    };
    too_large(s.set_u32(Sample::SCRIPT_LEN, 10_001));
    too_large(s.set_u32(Sample::COUNT, MAX_SALE_TRANSACTIONS as u32 + 1));
    too_large(s.set_u32(Sample::TX_LEN, MAX_TX_BYTES as u32 + 1));
    too_large(s.set_u32(s.n_groups(), MAX_ENTRY_GROUPS as u32 + 1));
    too_large(s.set_u32(s.n_blocks(), MAX_ENTRY_BLOCKS as u32 + 1));
    too_large(s.set_u32(s.n_confirmed(), MAX_SALE_TRANSACTIONS as u32 + 1));
    too_large(s.set_u32(s.n_replaced(), 65));
    non_canonical(&s.with(|b| {
        b[Sample::TOLERANCE..Sample::TOLERANCE + 4].copy_from_slice(&10_001u32.to_le_bytes())
    }));
    // At the caps they are not too large (they are short of bytes instead).
    for (at, max) in [
        (Sample::COUNT, MAX_SALE_TRANSACTIONS),
        (s.n_groups(), MAX_ENTRY_GROUPS),
        (s.n_blocks(), MAX_ENTRY_BLOCKS),
        (s.n_replaced(), 64),
    ] {
        assert_eq!(
            SaleWatch::from_bytes(&s.set_u32(at, max as u32)).err(),
            Some(DecodeError::Truncated),
            "{at}"
        );
    }
}

#[test]
fn flags_are_zero_or_one() {
    let s = Sample::new();
    for at in [
        Sample::CONFLICT,
        Sample::NEEDS,
        s.by_box(),
        s.first_seen_flag(),
    ] {
        non_canonical(&s.with(|b| b[at] = 2));
    }
}

#[test]
fn network_groups_must_be_valid_and_strictly_ascending() {
    let s = Sample::new();
    let g = s.groups();
    let (first, second) = (s.bytes[g..g + 5].to_vec(), s.bytes[g + 5..g + 10].to_vec());
    assert!(first < second);
    // swapped, and repeated
    non_canonical(&s.with(|b| {
        b[g..g + 5].copy_from_slice(&second);
        b[g + 5..g + 10].copy_from_slice(&first);
    }));
    non_canonical(&s.with(|b| b[g + 5..g + 10].copy_from_slice(&first)));
    // an unknown kind, and padding that is not zero
    non_canonical(&s.with(|b| b[g + 5] = 9));
    non_canonical(&s.with(|b| {
        b[g..g + 5].copy_from_slice(&[0, 1, 0, 0, 0]);
    }));
    // a private prefix, which `network_group` never gives
    non_canonical(&s.with(|b| b[g + 5..g + 10].copy_from_slice(&[2, 192, 168, 0, 0])));
}

#[test]
fn blocks_must_be_strictly_ascending() {
    let s = Sample::new();
    let with_blocks = |blocks: &[[u8; 32]]| {
        s.with(|b| {
            let at = s.n_blocks();
            b[at..at + 4].copy_from_slice(&(blocks.len() as u32).to_le_bytes());
            let flat: Vec<u8> = blocks.concat();
            b.splice(at + 4..at + 4, flat);
        })
    };
    assert!(SaleWatch::from_bytes(&with_blocks(&[[1; 32], [2; 32]])).is_ok());
    non_canonical(&with_blocks(&[[2; 32], [1; 32]]));
    non_canonical(&with_blocks(&[[1; 32], [1; 32]]));
}

#[test]
fn the_first_seen_time_must_fit_the_evidence() {
    let s = Sample::new();
    // Reported by the Box, so there has to be a time.
    non_canonical(&s.with(|b| {
        let at = s.first_seen_flag();
        b[at] = 0;
        b.drain(at + 1..at + 9);
    }));
    // Not reported by the Box and seen by two groups: no time yet.
    non_canonical(&s.with(|b| b[s.by_box()] = 0));
    // After the latest sighting.
    non_canonical(
        &s.with(|b| b[s.first_seen()..s.first_seen() + 8].copy_from_slice(&u64::MAX.to_le_bytes())),
    );
    let latest = u64::from_le_bytes(
        s.bytes[Sample::LATEST..Sample::LATEST + 8]
            .try_into()
            .unwrap(),
    );
    assert_eq!(latest, NOW + 5);
    let later = s.with(|b| {
        b[s.first_seen()..s.first_seen() + 8].copy_from_slice(&(latest + 1).to_le_bytes())
    });
    non_canonical(&later);
}

#[test]
fn the_sets_after_the_transactions_are_checked() {
    let s = Sample::new();
    let set = |at: usize, ids: &[[u8; 32]]| {
        s.with(|b| {
            b[at..at + 4].copy_from_slice(&(ids.len() as u32).to_le_bytes());
            b.splice(at + 4..at + 4, ids.concat());
        })
    };
    // A confirmed transaction has to be one of the sale.
    non_canonical(&set(s.n_confirmed(), &[[7; 32]]));
    let own = pays(1, 10_000).compute_txid().to_byte_array();
    assert!(SaleWatch::from_bytes(&set(s.n_confirmed(), &[own])).is_ok());
    // Replaced transactions: any, but ascending.
    assert!(SaleWatch::from_bytes(&set(s.n_replaced(), &[[1; 32], [2; 32]])).is_ok());
    non_canonical(&set(s.n_replaced(), &[[2; 32], [1; 32]]));
    non_canonical(&set(s.n_replaced(), &[[1; 32], [1; 32]]));
}

#[test]
fn spends_elsewhere_must_be_strictly_ascending() {
    let s = Sample::new();
    let record = |n: u8| {
        let mut r = [n; 100];
        r[32..36].copy_from_slice(&0u32.to_le_bytes());
        r
    };
    let with = |records: &[[u8; 100]]| {
        s.with(|b| {
            let at = s.n_elsewhere();
            b[at..at + 4].copy_from_slice(&(records.len() as u32).to_le_bytes());
            b.splice(at + 4..at + 4, records.concat());
        })
    };
    assert!(SaleWatch::from_bytes(&with(&[record(1), record(2)])).is_ok());
    non_canonical(&with(&[record(2), record(1)]));
    non_canonical(&with(&[record(1), record(1)]));
}

#[test]
fn the_script_is_stored_whole() {
    let mut w = SaleWatch::new(ScriptBuf::new(), 1, 0);
    let back = SaleWatch::from_bytes(&w.to_bytes()).unwrap();
    assert!(back.script().is_empty());
    w = SaleWatch::new(ScriptBuf::from_bytes(vec![0x6a; 10_000]), 1, 0);
    let back = SaleWatch::from_bytes(&w.to_bytes()).unwrap();
    assert_eq!(back.script(), w.script());
}

#[test]
fn the_time_a_block_was_learned_round_trips_and_needs_a_block() {
    let mut w = SaleWatch::new(shop(), 10_000, 0);
    let tx = pays(1, 10_000);
    let mut chain = TestChain::new(5);
    let b = block_at(vec![coinbase(5), tx.clone()], 5);
    chain.add(b.block_hash(), 5);
    w.confirmed_at(proven(&b, 1, 5), 321).unwrap();
    let mut back = round_trip(&w, &chain, NOW);
    status_at(&mut back, &chain, NOW);
    assert_eq!(back.standing_first_seen(), Some(321));
    // The standing payments are worked out again, not stored.
    let fresh = SaleWatch::from_bytes(&w.to_bytes()).unwrap();
    assert!(fresh.standing().is_empty());
    // A time with no block behind it is refused.
    let mut v = SaleWatch::new(shop(), 10_000, 0);
    sight(&mut v, &tx, peer(20), NOW);
    let mut saved = v.to_bytes();
    let last = saved.len();
    // The entry ends with its blocks (u32 count = 0) and the flag byte of
    // the time; the sets after it are three empty counts (12 bytes).
    let flag_at = last - 12 - 1;
    assert_eq!(saved[flag_at], 0);
    saved[flag_at] = 1;
    saved.splice(flag_at + 1..flag_at + 1, 5u64.to_le_bytes());
    assert!(SaleWatch::from_bytes(&saved).is_err());
}

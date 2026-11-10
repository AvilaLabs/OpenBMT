//! WR-04 to WR-08: the header store, on real mainnet headers and on
//! regtest headers mined by the tests.

use bitcoin::block::{Header, Version as BlockVersion};
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, CompactTarget, Network, TxMerkleNode};
use openbmt_core::wire::DecodeError;

use crate::headers::{MAINNET_CHECKPOINT, MAX_SIDE_HEADERS};
use crate::{Accepted, HeaderChain, HeaderStore, Refusal};

const REAL: &[u8] = include_bytes!("data/mainnet-headers-967680.bin");
const CHECKPOINT_HASH: &str = "0000000000000000000161354016172f2a377610d52dd46d0f68a919333458cd";
const LATE: u64 = 4_000_000_000;

/// The 4,043 real headers, heights 967,680 to 971,722.
fn real() -> Vec<Header> {
    REAL.chunks_exact(80)
        .map(|raw| deserialize(raw).expect("80 bytes are a header"))
        .collect()
}

/// A mainnet store holding the real headers up to and including the one at
/// this height.
fn real_store_to(height: u32) -> (HeaderStore, Vec<Header>) {
    let headers = real();
    let mut store = HeaderStore::new(Network::Bitcoin).expect("mainnet");
    for h in headers.iter().skip(1).take((height - 967_680) as usize) {
        assert_eq!(store.accept(h, LATE), Ok(Accepted::Extended));
    }
    (store, headers)
}

fn real_at(headers: &[Header], height: u32) -> Header {
    headers[(height - 967_680) as usize]
}

/// A regtest header on this parent, mined until it meets its own target.
/// `tag` makes headers with one parent and one time different.
fn mine(parent: &Header, time: u32, tag: u32) -> Header {
    mine_on(parent.block_hash(), parent.bits, time, tag)
}

fn mine_on(parent: BlockHash, bits: CompactTarget, time: u32, tag: u32) -> Header {
    let mut root = [0u8; 32];
    root[..4].copy_from_slice(&tag.to_le_bytes());
    let mut header = Header {
        version: BlockVersion::ONE,
        prev_blockhash: parent,
        merkle_root: TxMerkleNode::from_byte_array(root),
        time,
        bits,
        nonce: 0,
    };
    while header.validate_pow(header.target()).is_err() {
        header.nonce += 1;
    }
    header
}

fn regtest() -> (HeaderStore, Header) {
    let store = HeaderStore::new(Network::Regtest).expect("regtest");
    (store, genesis_block(Network::Regtest).header)
}

/// Regtest blocks on top of `from`, one second apart, accepted in order.
fn extend(store: &mut HeaderStore, from: &Header, n: u32, tag: u32) -> Vec<Header> {
    let mut out: Vec<Header> = Vec::new();
    let mut parent = *from;
    for _ in 0..n {
        let header = mine(&parent, parent.time + 1, tag);
        store.accept(&header, LATE).expect("accepted");
        out.push(header);
        parent = header;
    }
    out
}

// WR-04

#[test]
fn wr_04_checkpoint_constant_hashes_to_the_stated_value() {
    let header: Header = deserialize(&MAINNET_CHECKPOINT).expect("header");
    assert_eq!(header.block_hash().to_string(), CHECKPOINT_HASH);
    assert_eq!(header.bits.to_consensus(), 0x1702_1ec5);
    assert_eq!(header.time, 1_789_801_745);
    assert_eq!(&REAL[..80], &MAINNET_CHECKPOINT[..]);
}

#[test]
fn wr_04_mainnet_starts_at_the_checkpoint() {
    let store = HeaderStore::new(Network::Bitcoin).expect("mainnet");
    assert_eq!(store.start_height(), 967_680);
    assert_eq!(store.start_height() % 2_016, 0);
    assert_eq!(store.tip().0, 967_680);
    assert_eq!(store.tip().1.to_string(), CHECKPOINT_HASH);
    assert_eq!(store.height_of(&store.tip().1), Some(967_680));
    assert!(store.header_at(967_679).is_none());
    assert!(store.header_at(967_681).is_none());
}

#[test]
fn wr_04_signet_and_regtest_start_at_genesis() {
    for network in [Network::Signet, Network::Regtest] {
        let store = HeaderStore::new(network).expect("store");
        let genesis = genesis_block(network).header;
        assert_eq!(store.start_height(), 0);
        assert_eq!(store.tip(), (0, genesis.block_hash()));
        assert_eq!(store.header_at(0), Some(&genesis));
        assert_eq!(store.network(), network);
    }
}

#[test]
fn wr_04_testnet_and_testnet4_are_refused() {
    for network in [Network::Testnet, Network::Testnet4] {
        let refusal = HeaderStore::new(network).expect_err("refused");
        assert_eq!(refusal, Refusal::NetworkNotSupported);
        assert_eq!(refusal.rule(), "WR-04");
        assert_eq!(
            HeaderStore::from_bytes(network, &[]).expect_err("refused"),
            Refusal::NetworkNotSupported
        );
    }
}

// WR-05

#[test]
fn wr_05_all_real_headers_are_accepted_in_order() {
    let headers = real();
    assert_eq!(headers.len(), 4_043);
    let mut store = HeaderStore::new(Network::Bitcoin).expect("mainnet");
    for h in headers.iter().skip(1) {
        assert_eq!(store.accept(h, LATE), Ok(Accepted::Extended));
    }
    assert_eq!(store.tip().0, 971_722);
    assert_eq!(store.tip().1, headers[4_042].block_hash());
    assert_eq!(store.tip_height(), 971_722);
    assert_eq!(store.tip_time(), u64::from(headers[4_042].time));
    assert_eq!(store.height_of(&headers[0].block_hash()), Some(967_680));
    assert_eq!(store.height_of(&headers[4_042].block_hash()), Some(971_722));
    assert_eq!(store.header_at(969_696), Some(&headers[2_016]));
    // The same header again is known, and nothing changes.
    assert_eq!(store.accept(&headers[7], LATE), Ok(Accepted::Known));
    assert_eq!(store.tip().0, 971_722);
}

#[test]
fn wr_05_the_real_retargets_are_computed_exactly() {
    // 969,696: 17021ec5 -> 17021ef0. 971,712: 17021ef0 -> 17020093.
    for (height, old, new) in [
        (969_696, 0x1702_1ec5u32, 0x1702_1ef0u32),
        (971_712, 0x1702_1ef0, 0x1702_0093),
    ] {
        let (mut store, headers) = real_store_to(height - 1);
        let real = real_at(&headers, height);
        assert_eq!(real.bits.to_consensus(), new);
        assert_eq!(real_at(&headers, height - 1).bits.to_consensus(), old);

        // Bits changed by one: refused under the bits rule, with the real
        // bits as the required ones, although the work is also wrong.
        let mut off_by_one = real;
        off_by_one.bits = CompactTarget::from_consensus(new + 1);
        let refusal = store.accept(&off_by_one, LATE).expect_err("refused");
        assert_eq!(
            refusal,
            Refusal::WrongBits {
                expected: new,
                found: new + 1
            }
        );
        assert_eq!(refusal.rule(), "WR-05");

        // The old period's bits are refused too.
        let mut unchanged = real;
        unchanged.bits = CompactTarget::from_consensus(old);
        assert_eq!(
            store.accept(&unchanged, LATE),
            Err(Refusal::WrongBits {
                expected: new,
                found: old
            })
        );
        assert_eq!(store.tip().0, height - 1);

        // The real header is accepted.
        assert_eq!(store.accept(&real, LATE), Ok(Accepted::Extended));
    }
}

#[test]
fn wr_05_bits_inside_a_period_must_be_the_parents() {
    let (mut store, headers) = real_store_to(967_700);
    let mut next = real_at(&headers, 967_701);
    next.bits = CompactTarget::from_consensus(0x1702_1ef0);
    assert_eq!(
        store.accept(&next, LATE),
        Err(Refusal::WrongBits {
            expected: 0x1702_1ec5,
            found: 0x1702_1ef0
        })
    );
    assert_eq!(store.tip().0, 967_700);
}

#[test]
fn wr_05_a_header_with_a_bad_nonce_is_refused_for_its_work() {
    let (mut store, headers) = real_store_to(967_700);
    let mut next = real_at(&headers, 967_701);
    next.nonce ^= 1;
    let refusal = store.accept(&next, LATE).expect_err("refused");
    assert_eq!(refusal, Refusal::ChainProofOfWork);
    assert_eq!(refusal.rule(), "WR-05");
    assert!(!store.holds(&next.block_hash()));
}

#[test]
fn wr_05_a_header_with_an_unknown_parent_is_refused() {
    let (mut store, headers) = real_store_to(967_700);
    let refusal = store
        .accept(&real_at(&headers, 967_702), LATE)
        .expect_err("refused");
    assert_eq!(refusal, Refusal::UnknownParent);
    assert_eq!(refusal.rule(), "WR-05");
}

#[test]
fn wr_05_the_median_time_rule_with_eleven_and_with_fewer_headers() {
    let (mut store, genesis) = regtest();
    // Fewer than 11: genesis and three blocks, times t0, +10, +20, +30. The
    // median of the four is the third smallest, t0 + 20.
    let t0 = genesis.time;
    let mut parent = genesis;
    for i in 1..=3u32 {
        let h = mine(&parent, t0 + 10 * i, 0);
        assert_eq!(store.accept(&h, LATE), Ok(Accepted::Extended));
        parent = h;
    }
    let at_median = mine(&parent, t0 + 20, 1);
    assert_eq!(
        store.accept(&at_median, LATE),
        Err(Refusal::TimeNotAfterMedian {
            median: t0 + 20,
            time: t0 + 20
        })
    );
    let below = mine(&parent, t0 + 5, 2);
    assert!(matches!(
        store.accept(&below, LATE),
        Err(Refusal::TimeNotAfterMedian { .. })
    ));
    let just_after = mine(&parent, t0 + 21, 3);
    assert_eq!(store.accept(&just_after, LATE), Ok(Accepted::Extended));

    // Eleven or more: 12 blocks whose times run 100 apart, then one whose
    // time is below the older ones but above the median of the last 11.
    let (mut store, genesis) = regtest();
    let mut parent = genesis;
    let mut times = vec![genesis.time];
    for i in 1..=12u32 {
        let h = mine(&parent, genesis.time + 100 * i, 0);
        store.accept(&h, LATE).expect("accepted");
        times.push(h.time);
        parent = h;
    }
    // The last 11 are blocks 2..=12, times +200..+1200; the median is +700.
    let median = genesis.time + 700;
    assert_eq!(
        store.accept(&mine(&parent, median, 1), LATE),
        Err(Refusal::TimeNotAfterMedian {
            median,
            time: median
        })
    );
    assert_eq!(
        store.accept(&mine(&parent, median + 1, 2), LATE),
        Ok(Accepted::Extended)
    );
}

#[test]
fn wr_05_the_median_is_taken_on_the_headers_own_branch() {
    let (mut store, genesis) = regtest();
    let t0 = genesis.time;
    // The best chain runs far ahead in time.
    let mut parent = genesis;
    for i in 1..=6u32 {
        let h = mine(&parent, t0 + 10_000 * i, 0);
        store.accept(&h, LATE).expect("accepted");
        parent = h;
    }
    // A branch from genesis with small times is judged by its own median.
    let b1 = mine(&genesis, t0 + 1, 1);
    let b2 = mine(&b1, t0 + 2, 1);
    assert_eq!(store.accept(&b1, LATE), Ok(Accepted::Side));
    assert_eq!(store.accept(&b2, LATE), Ok(Accepted::Side));
    let bad = mine(&b2, t0 + 1, 2);
    assert!(matches!(
        store.accept(&bad, LATE),
        Err(Refusal::TimeNotAfterMedian { .. })
    ));
}

#[test]
fn wr_05_checks_run_in_the_stated_order() {
    let (mut store, genesis) = regtest();
    // Unknown parent beats everything else.
    let mut orphan = mine(&genesis, genesis.time + 1, 0);
    orphan.prev_blockhash = BlockHash::from_byte_array([9; 32]);
    orphan.bits = CompactTarget::from_consensus(0x1d00_ffff);
    assert_eq!(store.accept(&orphan, 0), Err(Refusal::UnknownParent));
    // Bits come before work: wrong bits and unmined.
    let mut bad = mine(&genesis, genesis.time + 1, 0);
    bad.bits = CompactTarget::from_consensus(0x1d00_ffff);
    assert!(matches!(
        store.accept(&bad, 0),
        Err(Refusal::WrongBits { .. })
    ));
    // Work comes before time: a time in the past and an unmined header.
    let mut unmined = mine(&genesis, genesis.time + 1, 0);
    unmined.time = genesis.time;
    unmined.nonce = 0;
    while unmined.validate_pow(unmined.target()).is_ok() {
        unmined.nonce += 1;
    }
    assert_eq!(store.accept(&unmined, 0), Err(Refusal::ChainProofOfWork));
    // Time comes before the clock.
    let old = mine(&genesis, genesis.time, 0);
    assert!(matches!(
        store.accept(&old, 0),
        Err(Refusal::TimeNotAfterMedian { .. })
    ));
}

// WR-06

#[test]
fn wr_06_a_header_too_far_ahead_is_refused_for_now_and_accepted_later() {
    let (mut store, headers) = real_store_to(967_700);
    let next = real_at(&headers, 967_701);
    let time = u64::from(next.time);
    let before = store.clone().to_bytes();

    let refusal = store.accept(&next, time - 7_201).expect_err("refused");
    assert_eq!(refusal, Refusal::TimeTooFarAhead { time: next.time });
    assert_eq!(refusal.rule(), "WR-06");
    // Nothing was stored.
    assert!(!store.holds(&next.block_hash()));
    assert_eq!(store.to_bytes(), before);
    assert_eq!(store.tip().0, 967_700);

    // Exactly 7,200 ahead is allowed.
    let mut again = store.clone();
    assert_eq!(again.accept(&next, time - 7_200), Ok(Accepted::Extended));
    // Offered again later, the same header is accepted.
    assert_eq!(store.accept(&next, time + 60), Ok(Accepted::Extended));
    assert_eq!(store.tip().0, 967_701);
}

#[test]
fn wr_06_a_bad_header_is_reported_as_bad_whatever_its_time() {
    let (mut store, headers) = real_store_to(967_700);
    let mut next = real_at(&headers, 967_701);
    next.nonce ^= 1;
    assert_eq!(store.accept(&next, 0), Err(Refusal::ChainProofOfWork));
}

// WR-07

#[test]
fn wr_07_more_work_wins_and_the_blocks_that_left_are_listed() {
    let (mut store, genesis) = regtest();
    let a = extend(&mut store, &genesis, 3, 0);
    let a_hashes: Vec<BlockHash> = a.iter().map(Header::block_hash).collect();
    // A branch from genesis: equal work does not replace the best chain.
    let mut parent = genesis;
    let mut b = Vec::new();
    for _ in 0..3 {
        let h = mine(&parent, parent.time + 1, 7);
        assert_eq!(store.accept(&h, LATE), Ok(Accepted::Side));
        parent = h;
        b.push(h);
    }
    assert_eq!(store.tip(), (3, a_hashes[2]));
    assert_eq!(store.height_of(&a_hashes[2]), Some(3));
    assert_eq!(store.height_of(&b[2].block_hash()), None);
    assert_eq!(store.side_count(), 3);
    // One more block on the branch: more work.
    let b4 = mine(&parent, parent.time + 1, 7);
    let outcome = store.accept(&b4, LATE).expect("accepted");
    assert_eq!(
        outcome,
        Accepted::Reorganized {
            left: vec![a_hashes[2], a_hashes[1], a_hashes[0]]
        }
    );
    assert_eq!(store.tip(), (4, b4.block_hash()));
    for h in &a_hashes {
        assert_eq!(store.height_of(h), None);
    }
    for (i, h) in b.iter().enumerate() {
        assert_eq!(store.height_of(&h.block_hash()), Some(i as u32 + 1));
    }
    assert_eq!(store.header_at(2), Some(&b[1]));
    // The old blocks are held beside the chain now, and can win it back.
    assert!(store.holds(&a_hashes[0]));
    let a4 = mine(&a[2], a[2].time + 1, 0);
    let a5 = mine(&a4, a4.time + 1, 0);
    assert_eq!(store.accept(&a4, LATE), Ok(Accepted::Side));
    assert_eq!(
        store.accept(&a5, LATE),
        Ok(Accepted::Reorganized {
            left: vec![
                b4.block_hash(),
                b[2].block_hash(),
                b[1].block_hash(),
                b[0].block_hash()
            ]
        })
    );
    assert_eq!(store.height_of(&a_hashes[0]), Some(1));
}

#[test]
fn wr_07_a_reorganization_part_way_down_keeps_the_common_blocks() {
    let (mut store, genesis) = regtest();
    let trunk = extend(&mut store, &genesis, 2, 0);
    let a = extend(&mut store, &trunk[1], 2, 1);
    let b1 = mine(&trunk[1], trunk[1].time + 1, 2);
    let b2 = mine(&b1, b1.time + 1, 2);
    let b3 = mine(&b2, b2.time + 1, 2);
    assert_eq!(store.accept(&b1, LATE), Ok(Accepted::Side));
    assert_eq!(store.accept(&b2, LATE), Ok(Accepted::Side));
    assert_eq!(
        store.accept(&b3, LATE),
        Ok(Accepted::Reorganized {
            left: vec![a[1].block_hash(), a[0].block_hash()]
        })
    );
    assert_eq!(store.height_of(&trunk[1].block_hash()), Some(2));
    assert_eq!(store.tip(), (5, b3.block_hash()));
}

#[test]
fn wr_07_a_repeat_is_known_and_changes_nothing() {
    let (mut store, genesis) = regtest();
    let a = extend(&mut store, &genesis, 2, 0);
    let side = mine(&genesis, genesis.time + 1, 5);
    assert_eq!(store.accept(&side, LATE), Ok(Accepted::Side));
    let bytes = store.to_bytes();
    assert_eq!(store.accept(&a[0], LATE), Ok(Accepted::Known));
    assert_eq!(store.accept(&a[1], 0), Ok(Accepted::Known));
    assert_eq!(store.accept(&side, LATE), Ok(Accepted::Known));
    assert_eq!(store.to_bytes(), bytes);
    assert_eq!(store.side_count(), 1);
}

#[test]
fn wr_07_at_most_4032_headers_are_held_beside_the_chain() {
    let (mut store, genesis) = regtest();
    let a = extend(&mut store, &genesis, 1, 0);
    for tag in 1..=MAX_SIDE_HEADERS as u32 {
        let h = mine(&genesis, genesis.time + 1, tag);
        assert_eq!(store.accept(&h, LATE), Ok(Accepted::Side));
    }
    assert_eq!(store.side_count(), 4_032);
    let one_more = mine(&genesis, genesis.time + 1, 9_999);
    let refusal = store.accept(&one_more, LATE).expect_err("refused");
    assert_eq!(refusal, Refusal::SideHeadersFull);
    assert_eq!(refusal.rule(), "WR-07");
    assert!(!store.holds(&one_more.block_hash()));
    // A header that extends the best chain is not a side header.
    let next = mine(&a[0], a[0].time + 1, 0);
    assert_eq!(store.accept(&next, LATE), Ok(Accepted::Extended));
}

#[test]
fn wr_07_a_header_whose_parent_is_too_deep_is_refused_and_old_ones_pruned() {
    let (mut store, genesis) = regtest();
    // The best chain at height 2, a side header at height 1 and one on top
    // of it at height 2.
    let a1 = mine(&genesis, genesis.time + 1, 0);
    let a2 = mine(&a1, a1.time + 1, 0);
    store.accept(&a1, LATE).expect("accepted");
    store.accept(&a2, LATE).expect("accepted");
    let s1 = mine(&genesis, genesis.time + 1, 1);
    let s2 = mine(&s1, s1.time + 1, 1);
    assert_eq!(store.accept(&s1, LATE), Ok(Accepted::Side));
    assert_eq!(store.accept(&s2, LATE), Ok(Accepted::Side));

    // Grow the best chain to height 2,017: heights 1 and 2 are still within
    // 2,016 of the tip.
    let mut parent = a2;
    for _ in 3..=2_017 {
        let h = mine(&parent, parent.time + 1, 0);
        store.accept(&h, LATE).expect("accepted");
        parent = h;
    }
    assert_eq!(store.tip().0, 2_017);
    assert!(store.holds(&s1.block_hash()) && store.holds(&s2.block_hash()));
    // At 2,018 the header at height 1 goes, and with it the one on top.
    let h = mine(&parent, parent.time + 1, 0);
    store.accept(&h, LATE).expect("accepted");
    assert_eq!(store.tip().0, 2_018);
    assert!(!store.holds(&s1.block_hash()));
    assert!(!store.holds(&s2.block_hash()));
    assert_eq!(store.side_count(), 0);

    // A child of genesis has a parent 2,018 below the tip.
    let deep = mine(&genesis, genesis.time + 1, 3);
    let refusal = store.accept(&deep, LATE).expect_err("refused");
    assert_eq!(refusal, Refusal::SideHeaderTooDeep);
    assert_eq!(refusal.rule(), "WR-07");
    // A child of the block at height 2 has a parent exactly 2,016 below.
    let edge_parent = store.header_at(2).copied().expect("held");
    let edge = mine(&edge_parent, edge_parent.time + 1, 3);
    assert_eq!(store.accept(&edge, LATE), Ok(Accepted::Side));
    // A child of the block at height 1 is one too deep.
    let one_down = store.header_at(1).copied().expect("held");
    let too_deep = mine(&one_down, one_down.time + 1, 3);
    assert_eq!(
        store.accept(&too_deep, LATE),
        Err(Refusal::SideHeaderTooDeep)
    );
}

#[test]
fn wr_07_the_chain_view_answers_for_the_best_chain_only() {
    let (mut store, genesis) = regtest();
    let a = extend(&mut store, &genesis, 2, 0);
    let side = mine(&genesis, genesis.time + 1, 4);
    store.accept(&side, LATE).expect("accepted");
    assert_eq!(store.height_of(&genesis.block_hash()), Some(0));
    assert_eq!(store.height_of(&a[1].block_hash()), Some(2));
    assert_eq!(store.height_of(&side.block_hash()), None);
    assert_eq!(store.height_of(&BlockHash::from_byte_array([1; 32])), None);
    assert_eq!(store.tip_height(), 2);
    assert_eq!(store.tip_time(), u64::from(a[1].time));
    assert_eq!(crate::confirmations(&store, &genesis.block_hash()), Some(3));
}

#[test]
fn wr_07_the_locator_goes_by_single_steps_then_doubles_to_the_start() {
    let (mut store, genesis) = regtest();
    assert_eq!(store.locator(), vec![genesis.block_hash()]);
    let blocks = extend(&mut store, &genesis, 30, 0);
    let hash_at = |height: usize| {
        if height == 0 {
            genesis.block_hash()
        } else {
            blocks[height - 1].block_hash()
        }
    };
    let heights: Vec<usize> = (19..=30).rev().chain([17, 13, 5, 0]).collect();
    let want: Vec<BlockHash> = heights.into_iter().map(hash_at).collect();
    assert_eq!(store.locator(), want);

    // On mainnet it ends at the checkpoint.
    let (store, headers) = real_store_to(967_690);
    let locator = store.locator();
    assert_eq!(locator.len(), 11);
    assert_eq!(locator[0], real_at(&headers, 967_690).block_hash());
    assert_eq!(locator[10], headers[0].block_hash());
}

// WR-08

#[test]
fn wr_08_the_real_chain_round_trips() {
    let headers = real();
    let mut store = HeaderStore::new(Network::Bitcoin).expect("mainnet");
    for h in headers.iter().skip(1) {
        store.accept(h, LATE).expect("accepted");
    }
    let bytes = store.to_bytes();
    assert_eq!(bytes.len(), 4 + 1 + 1 + 32 + 4 + 4_042 * 80);
    assert_eq!(&bytes[..5], b"OBMW\x01");
    assert_eq!(bytes[5], 0);
    assert_eq!(&bytes[6..38], &headers[0].block_hash().to_byte_array());
    assert_eq!(&bytes[38..42], &4_042u32.to_le_bytes());
    assert_eq!(&bytes[42..], &REAL[80..]);
    let back = HeaderStore::from_bytes(Network::Bitcoin, &bytes).expect("read");
    assert_eq!(back.tip(), store.tip());
    assert_eq!(back.to_bytes(), bytes);
    assert_eq!(back.height_of(&headers[2_016].block_hash()), Some(969_696));

    // An empty store round trips too.
    let empty = HeaderStore::new(Network::Bitcoin).expect("mainnet");
    let back = HeaderStore::from_bytes(Network::Bitcoin, &empty.to_bytes()).expect("read");
    assert_eq!(back.tip().0, 967_680);
}

#[test]
fn wr_08_side_headers_are_not_stored() {
    let (mut store, genesis) = regtest();
    extend(&mut store, &genesis, 2, 0);
    let bytes = store.to_bytes();
    let side = mine(&genesis, genesis.time + 1, 8);
    store.accept(&side, LATE).expect("accepted");
    assert_eq!(store.to_bytes(), bytes);
    let back = HeaderStore::from_bytes(Network::Regtest, &bytes).expect("read");
    assert!(!back.holds(&side.block_hash()));
}

#[test]
fn wr_08_a_byte_more_or_less_is_refused() {
    let (mut store, genesis) = regtest();
    extend(&mut store, &genesis, 5, 0);
    let bytes = store.to_bytes();
    // Every truncation, down to nothing.
    for len in 0..bytes.len() {
        let refusal = HeaderStore::from_bytes(Network::Regtest, &bytes[..len]).expect_err("cut");
        assert_eq!(refusal.rule(), "WR-08", "{len}");
    }
    // One byte more.
    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(
        HeaderStore::from_bytes(Network::Regtest, &longer),
        Err(Refusal::StoreMalformed(DecodeError::TrailingBytes))
    );
    // The same on the real chain.
    let headers = real();
    let mut real_store = HeaderStore::new(Network::Bitcoin).expect("mainnet");
    for h in headers.iter().skip(1) {
        real_store.accept(h, LATE).expect("accepted");
    }
    let real_bytes = real_store.to_bytes();
    let cut = &real_bytes[..real_bytes.len() - 1];
    assert_eq!(
        HeaderStore::from_bytes(Network::Bitcoin, cut),
        Err(Refusal::StoreMalformed(DecodeError::Truncated))
    );
    let mut longer = real_bytes;
    longer.push(0);
    assert_eq!(
        HeaderStore::from_bytes(Network::Bitcoin, &longer),
        Err(Refusal::StoreMalformed(DecodeError::TrailingBytes))
    );
}

#[test]
fn wr_08_wrong_magic_version_network_or_start_is_refused() {
    let (mut store, genesis) = regtest();
    extend(&mut store, &genesis, 2, 0);
    let bytes = store.to_bytes();

    let mut bad = bytes.clone();
    bad[0] ^= 1;
    assert_eq!(
        HeaderStore::from_bytes(Network::Regtest, &bad),
        Err(Refusal::StoreMalformed(DecodeError::BadMagic))
    );
    let mut bad = bytes.clone();
    bad[4] = 2;
    assert_eq!(
        HeaderStore::from_bytes(Network::Regtest, &bad),
        Err(Refusal::StoreMalformed(DecodeError::UnsupportedVersion(2)))
    );
    // Another network's byte, and one nobody uses.
    for byte in [0u8, 1, 3, 255] {
        let mut bad = bytes.clone();
        bad[5] = byte;
        assert_eq!(
            HeaderStore::from_bytes(Network::Regtest, &bad),
            Err(Refusal::StoreWrongNetwork)
        );
    }
    // Read as another network.
    assert_eq!(
        HeaderStore::from_bytes(Network::Signet, &bytes),
        Err(Refusal::StoreWrongNetwork)
    );
    let mut bad = bytes.clone();
    bad[6] ^= 1;
    assert_eq!(
        HeaderStore::from_bytes(Network::Regtest, &bad),
        Err(Refusal::StoreWrongStart)
    );
    let refusal = Refusal::StoreWrongStart;
    assert_eq!(refusal.rule(), "WR-08");
}

#[test]
fn wr_08_an_altered_header_refuses_the_whole_file() {
    let headers = real();
    let mut store = HeaderStore::new(Network::Bitcoin).expect("mainnet");
    for h in headers.iter().skip(1) {
        store.accept(h, LATE).expect("accepted");
    }
    let bytes = store.to_bytes();
    let at = |index: usize| 42 + index * 80;

    // Nonce altered (bytes 76..80 of the header).
    let mut bad = bytes.clone();
    bad[at(100) + 76] ^= 1;
    match HeaderStore::from_bytes(Network::Bitcoin, &bad).expect_err("refused") {
        Refusal::StoreHeaderRefused { index, why } => {
            assert_eq!(index, 100);
            assert_eq!(*why, Refusal::ChainProofOfWork);
        }
        other => panic!("{other:?}"),
    }
    // Bits altered inside a period.
    let mut bad = bytes.clone();
    bad[at(50) + 72] ^= 1;
    match HeaderStore::from_bytes(Network::Bitcoin, &bad).expect_err("refused") {
        Refusal::StoreHeaderRefused { index, why } => {
            assert_eq!(index, 50);
            assert!(matches!(*why, Refusal::WrongBits { .. }));
        }
        other => panic!("{other:?}"),
    }
    // Bits altered at a retarget (index 2,015 is height 969,696).
    let mut bad = bytes.clone();
    bad[at(2_015) + 72] ^= 1;
    match HeaderStore::from_bytes(Network::Bitcoin, &bad).expect_err("refused") {
        Refusal::StoreHeaderRefused { index, why } => {
            assert_eq!(index, 2_015);
            assert!(matches!(*why, Refusal::WrongBits { .. }));
        }
        other => panic!("{other:?}"),
    }
    let refusal = HeaderStore::from_bytes(Network::Bitcoin, &bad).expect_err("refused");
    assert_eq!(refusal.rule(), "WR-08");
    assert!(refusal.to_string().starts_with("WR-08: "));
}

#[test]
fn wr_08_a_gap_or_a_reordering_is_refused() {
    let headers = real();
    let mut store = HeaderStore::new(Network::Bitcoin).expect("mainnet");
    for h in headers.iter().skip(1).take(10) {
        store.accept(h, LATE).expect("accepted");
    }
    let bytes = store.to_bytes();
    // Cut out the header at index 3 and keep the count in step.
    let mut gap = bytes.clone();
    gap.drain(42 + 3 * 80..42 + 4 * 80);
    gap[38..42].copy_from_slice(&9u32.to_le_bytes());
    match HeaderStore::from_bytes(Network::Bitcoin, &gap).expect_err("refused") {
        Refusal::StoreHeaderRefused { index, why } => {
            assert_eq!(index, 3);
            assert_eq!(*why, Refusal::StoreNotChained);
        }
        other => panic!("{other:?}"),
    }
    // A header repeated.
    let mut twice = bytes.clone();
    let copy: Vec<u8> = bytes[42..42 + 80].to_vec();
    twice.splice(42 + 80..42 + 80, copy);
    twice[38..42].copy_from_slice(&11u32.to_le_bytes());
    assert!(matches!(
        HeaderStore::from_bytes(Network::Bitcoin, &twice),
        Err(Refusal::StoreHeaderRefused { index: 1, .. })
    ));
}

#[test]
fn wr_08_a_count_larger_than_the_data_or_the_cap_is_refused() {
    let (mut store, genesis) = regtest();
    extend(&mut store, &genesis, 3, 0);
    let bytes = store.to_bytes();
    let mut bad = bytes.clone();
    bad[38..42].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        HeaderStore::from_bytes(Network::Regtest, &bad),
        Err(Refusal::StoreMalformed(DecodeError::Truncated))
    );
    // A count smaller than the data leaves bytes over.
    let mut bad = bytes.clone();
    bad[38..42].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        HeaderStore::from_bytes(Network::Regtest, &bad),
        Err(Refusal::StoreMalformed(DecodeError::TrailingBytes))
    );
    for count in [2_000_001u32, u32::MAX] {
        let mut bad = bytes.clone();
        bad[38..42].copy_from_slice(&count.to_le_bytes());
        assert_eq!(
            HeaderStore::from_bytes(Network::Regtest, &bad),
            Err(Refusal::StoreMalformed(DecodeError::TooLarge))
        );
    }
}

#[test]
fn wr_08_a_stored_header_that_is_not_on_the_tip_is_refused() {
    let (store, genesis) = regtest();
    let mut bytes = store.to_bytes();
    // One header whose parent is not the start block.
    let stray = mine_on(BlockHash::from_byte_array([3; 32]), genesis.bits, 5, 0);
    bytes.extend(serialize(&stray));
    bytes[38..42].copy_from_slice(&1u32.to_le_bytes());
    match HeaderStore::from_bytes(Network::Regtest, &bytes).expect_err("refused") {
        Refusal::StoreHeaderRefused { index: 0, why } => {
            assert_eq!(*why, Refusal::StoreNotChained);
        }
        other => panic!("{other:?}"),
    }
}

//! SW-70 to SW-74: following a swap by polling.

use bitcoin::absolute::LockTime;
use bitcoin::consensus::serialize;
use bitcoin::hashes::Hash;
use bitcoin::hex::DisplayHex;
use bitcoin::transaction::Version;
use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, Witness};

use super::fixtures::*;
use crate::follow::{Ctx, Failure, Next, ReverseSwap, Stage};
use crate::{
    Refused, Seen, Status, StatusKind, SwapState, MAGIC_FOLLOW, POLL_LOCKED_SECS, POLL_WAITING_SECS,
};

fn ctx() -> Ctx {
    Ctx {
        now_secs: NOW,
        tip_height: TIP,
        accept_unconfirmed: false,
    }
}

fn machine() -> ReverseSwap {
    ReverseSwap::new(SwapState::new(&good_swap(), &secrets(), NET, &destination()).unwrap())
}

fn lockup_tx(value: u64) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(Txid::from_byte_array([0xcd; 32]), 1),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(value),
            script_pubkey: good_swap().tree.script_pubkey(),
        }],
    }
}

fn status(name: &str, tx: Option<&Transaction>) -> Status {
    let body = match tx {
        Some(tx) => format!(
            "{{\"status\":\"{name}\",\"transaction\":{{\"id\":\"{}\",\"hex\":\"{}\"}}}}",
            tx.compute_txid(),
            serialize(tx).to_lower_hex_string()
        ),
        None => format!("{{\"status\":\"{name}\"}}"),
    };
    Status::parse(body.as_bytes()).unwrap()
}

fn waiting() -> Next {
    Next::Wait {
        poll_after_secs: POLL_WAITING_SECS,
    }
}

// SW-70

#[test]
fn sw_70_a_status_is_read_with_its_known_states_and_its_transaction() {
    for (name, kind) in [
        ("swap.created", StatusKind::SwapCreated),
        ("transaction.mempool", StatusKind::TransactionMempool),
        ("transaction.confirmed", StatusKind::TransactionConfirmed),
        ("invoice.settled", StatusKind::InvoiceSettled),
        ("invoice.expired", StatusKind::InvoiceExpired),
        ("transaction.failed", StatusKind::TransactionFailed),
        ("swap.expired", StatusKind::SwapExpired),
        ("transaction.refunded", StatusKind::TransactionRefunded),
    ] {
        assert_eq!(status(name, None).kind(), &kind);
    }
    // An unknown state is reported as unknown.
    let s = status("swap.invented", None);
    assert_eq!(s.kind(), &StatusKind::Unknown("swap.invented".to_string()));
    // The transaction, read at the two states that carry it.
    let tx = lockup_tx(99_350);
    let s = status("transaction.mempool", Some(&tx));
    assert_eq!(s.transaction(), Some(&tx));
    assert_eq!(s.txid(), Some(tx.compute_txid()));
    // The id alone is kept; the bytes alone give their id.
    let body = format!(
        "{{\"status\":\"transaction.confirmed\",\"transaction\":{{\"id\":\"{}\"}}}}",
        tx.compute_txid()
    );
    let s = Status::parse(body.as_bytes()).unwrap();
    assert_eq!((s.txid(), s.transaction()), (Some(tx.compute_txid()), None));
    let body = format!(
        "{{\"status\":\"transaction.mempool\",\"transaction\":{{\"hex\":\"{}\"}}}}",
        serialize(&tx).to_lower_hex_string()
    );
    assert_eq!(
        Status::parse(body.as_bytes()).unwrap().txid(),
        Some(tx.compute_txid())
    );
    // Other states do not read a transaction, and members the library does
    // not know are ignored.
    let s =
        Status::parse(b"{\"status\":\"swap.created\",\"transaction\":5,\"zeroConfRejected\":true}")
            .unwrap();
    assert_eq!((s.txid(), s.transaction()), (None, None));
    // Refusals.
    let wrong_id = format!(
        "{{\"status\":\"transaction.mempool\",\"transaction\":{{\"id\":\"{}\",\"hex\":\"{}\"}}}}",
        "11".repeat(32),
        serialize(&tx).to_lower_hex_string()
    );
    let long_name = format!("{{\"status\":\"{}\"}}", "a".repeat(65));
    let mut cases: Vec<&[u8]> = vec![
        b"{}",
        b"{\"status\":5}",
        b"{\"status\":\"\"}",
        b"{\"status\":\"has space\"}",
        b"{\"status\":\"transaction.mempool\",\"transaction\":[]}",
        b"{\"status\":\"transaction.mempool\",\"transaction\":{\"hex\":\"zz\"}}",
        b"{\"status\":\"transaction.mempool\",\"transaction\":{\"hex\":\"00\"}}",
        b"{\"status\":\"transaction.mempool\",\"transaction\":{\"id\":\"abc\"}}",
        b"[]",
        b"{\"status\":\"swap.created\"} x",
    ];
    cases.push(wrong_id.as_bytes());
    cases.push(long_name.as_bytes());
    for body in cases {
        let err = Status::parse(body).unwrap_err();
        assert_eq!(err.rule(), "SW-70", "{}", String::from_utf8_lossy(body));
    }
}

// SW-71

#[test]
fn sw_71_the_machine_says_what_to_do_next_and_every_change_is_a_new_value() {
    let m0 = machine();
    assert_eq!(m0.stage(), Stage::Created);
    // Nothing yet: wait.
    let (m1, next) = m0.on_status(&status("swap.created", None), &ctx());
    assert_eq!((m1.stage(), next), (Stage::Created, waiting()));
    // The service says there is a lockup and gives no bytes: check it.
    let (m2, next) = m1.on_status(&status("transaction.mempool", None), &ctx());
    assert_eq!((m2.stage(), next), (Stage::Announced, Next::CheckLockup));
    // The caller finds it, in the mempool: it is checked; the shop does not
    // claim an unconfirmed lockup unless its rule says so.
    let tx = lockup_tx(99_350);
    let (m3, next) = m2.on_lockup(&tx, Seen::Mempool, &ctx()).unwrap();
    let Stage::Locked(l) = m3.stage() else {
        panic!("{:?}", m3.stage())
    };
    assert!(!l.confirmed);
    assert_eq!(
        next,
        Next::Wait {
            poll_after_secs: POLL_LOCKED_SECS
        }
    );
    // The shop's rule accepts it: build the claim.
    let loose = Ctx {
        accept_unconfirmed: true,
        ..ctx()
    };
    assert_eq!(m3.settle(&loose).1, Next::BuildClaim { lockup: l });
    // In a block, it is claimed whatever the rule.
    let (m4, next) = m3.on_lockup(&tx, Seen::Block, &ctx()).unwrap();
    let Stage::Locked(l) = m4.stage() else {
        panic!()
    };
    assert!(l.confirmed);
    assert_eq!(next, Next::BuildClaim { lockup: l });
    // The claim: a new machine at "claim sent", the old one untouched.
    let (m5, claim) = m4.claim(2_000, &ctx()).unwrap();
    assert_eq!(m5.stage(), Stage::ClaimSent);
    assert_eq!(m4.stage(), Stage::Locked(l));
    assert_eq!(
        m5.state().phase(),
        crate::Phase::Claimed(claim.compute_txid())
    );
    assert_eq!(
        m5.settle(&ctx()).1,
        Next::WatchClaim {
            txid: claim.compute_txid()
        }
    );
    // Seen in a block: paid.
    let m6 = m5.claim_confirmed(claim.compute_txid(), TIP + 1).unwrap();
    assert_eq!(m6.stage(), Stage::Paid { height: TIP + 1 });
    assert_eq!(
        m6.settle(&ctx()).1,
        Next::Paid {
            txid: claim.compute_txid()
        }
    );
    // A claim is built from a checked lockup only.
    assert!(matches!(m0.claim(2_000, &ctx()), Err(Refused::NotReady(_))));
    assert_eq!(m0.claim(2_000, &ctx()).unwrap_err().rule(), "SW-71");
    // After the claim is sent a status changes nothing.
    let (same, next) = m5.on_status(&status("swap.expired", None), &ctx());
    assert_eq!(same, m5);
    assert!(matches!(next, Next::WatchClaim { .. }));
    // A status that says the swap failed ends it, with the reason, and no
    // claim follows.
    for (name, why) in [
        ("invoice.expired", Failure::InvoiceExpired),
        ("transaction.failed", Failure::LockupFailed),
        ("swap.expired", Failure::SwapExpired),
        ("transaction.refunded", Failure::Refunded),
    ] {
        let (f, next) = m4.on_status(&status(name, None), &ctx());
        assert_eq!((f.stage(), next), (Stage::Failed(why), Next::Failed(why)));
        assert!(f.claim(2_000, &ctx()).is_err());
    }
    // An unknown status is not guessed at, and a settled one tells nothing.
    for name in ["swap.invented", "invoice.settled"] {
        let (same, _) = m1.on_status(&status(name, None), &ctx());
        assert_eq!(same, m1);
    }
    // The clock and the chain end a swap too: at the timeout, or past the
    // invoice with no lockup.
    let late = Ctx {
        tip_height: TIMEOUT,
        ..ctx()
    };
    assert_eq!(m0.settle(&late).1, Next::Failed(Failure::TimedOut));
    assert_eq!(m4.settle(&late).1, Next::Failed(Failure::TimedOut));
    assert_eq!(m2.settle(&late).1, Next::Failed(Failure::TimedOut));
    let after_invoice = Ctx {
        now_secs: NOW + 4000,
        ..ctx()
    };
    assert_eq!(
        m0.settle(&after_invoice).1,
        Next::Failed(Failure::InvoiceExpired)
    );
    // A lockup that is in hand is still claimed after the invoice ended.
    assert!(matches!(
        m4.settle(&after_invoice).1,
        Next::BuildClaim { .. }
    ));
    // A restarted state that holds a claim starts at the claim.
    assert_eq!(
        ReverseSwap::new(m5.state().clone()).stage(),
        Stage::ClaimSent
    );
}

// SW-72

#[test]
fn sw_72_the_machine_believes_the_bytes_and_not_the_status_about_money() {
    let m = machine();
    // The bytes in a status are checked, and always taken as not in a
    // block, whatever the service says.
    let tx = lockup_tx(99_350);
    for name in ["transaction.mempool", "transaction.confirmed"] {
        let (m1, _) = m.on_status(&status(name, Some(&tx)), &ctx());
        let Stage::Locked(l) = m1.stage() else {
            panic!("{name}")
        };
        assert!(!l.confirmed);
        assert_eq!(l.value_sats, 99_350);
        assert_eq!(l.outpoint.txid, tx.compute_txid());
    }
    // Bytes the caller saw in a block are not taken back by a later status.
    let (in_block, _) = m.on_lockup(&tx, Seen::Block, &ctx()).unwrap();
    let (kept, next) = in_block.on_status(&status("transaction.mempool", Some(&tx)), &ctx());
    assert_eq!(kept, in_block);
    assert!(matches!(next, Next::BuildClaim { .. }));
    // A lockup that pays too little, or that pays something else, is not
    // believed: from the service's own status the swap ends and no claim
    // follows; from the caller's bytes nothing changes.
    let small = lockup_tx(99_349);
    let (f, next) = m.on_status(&status("transaction.mempool", Some(&small)), &ctx());
    assert_eq!(
        (f.stage(), next),
        (
            Stage::Failed(Failure::LockupRefused),
            Next::Failed(Failure::LockupRefused)
        )
    );
    assert_eq!(
        m.on_lockup(&small, Seen::Block, &ctx()).unwrap_err(),
        Refused::LockupTooSmall
    );
    let mut other = lockup_tx(99_350);
    other.output[0].script_pubkey = destination().script_pubkey();
    assert_eq!(
        m.on_lockup(&other, Seen::Mempool, &ctx()).unwrap_err(),
        Refused::NoLockupOutput
    );
    // No lockup is looked for after the claim.
    let (sent, _) = in_block.claim(1_000, &ctx()).unwrap();
    assert!(matches!(
        sent.on_lockup(&tx, Seen::Block, &ctx()),
        Err(Refused::NotReady(_))
    ));
    // Paid is the shop's own claim seen in a block, by its id, and only then.
    let txid = sent.state().phase();
    let crate::Phase::Claimed(txid) = txid else {
        panic!()
    };
    assert_eq!(
        sent.claim_confirmed(Txid::from_byte_array([1; 32]), TIP),
        Err(Refused::WrongClaim)
    );
    assert_eq!(Refused::WrongClaim.rule(), "SW-72");
    assert!(matches!(
        in_block.claim_confirmed(txid, TIP),
        Err(Refused::NotReady(_))
    ));
    // `invoice.settled` does not make the swap paid.
    let (not_paid, _) = sent.on_status(&status("invoice.settled", None), &ctx());
    assert_eq!(not_paid.stage(), Stage::ClaimSent);
}

// SW-73, SW-74

fn every_stage() -> Vec<ReverseSwap> {
    let m = machine();
    let tx = lockup_tx(99_350);
    let (announced, _) = m.on_status(&status("transaction.mempool", None), &ctx());
    let (mempool, _) = m.on_lockup(&tx, Seen::Mempool, &ctx()).unwrap();
    let (block, _) = m.on_lockup(&tx, Seen::Block, &ctx()).unwrap();
    let (sent, claim) = block.claim(1_500, &ctx()).unwrap();
    let paid = sent.claim_confirmed(claim.compute_txid(), 250_010).unwrap();
    let (failed, _) = m.on_status(&status("swap.expired", None), &ctx());
    vec![m, announced, mempool, block, sent, paid, failed]
}

#[test]
fn sw_73_the_machine_is_saved_and_restored_whole() {
    for m in every_stage() {
        let bytes = m.encode();
        assert_eq!(&bytes[..4], b"OBM'");
        assert_eq!(bytes[..4], MAGIC_FOLLOW);
        assert_eq!(bytes[4], 1);
        let back = ReverseSwap::decode(&bytes).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.encode(), bytes);
        // A short file and a long one, at every length.
        for n in 0..bytes.len() {
            assert!(ReverseSwap::decode(&bytes[..n]).is_err(), "{n}");
        }
        let mut long = bytes.clone();
        long.push(0);
        assert!(ReverseSwap::decode(&long).is_err());
    }
    // A lockup is not lost by a restart: the machine claims it after.
    let saved = every_stage()[3].encode();
    let back = ReverseSwap::decode(&saved).unwrap();
    assert!(back.claim(2_000, &ctx()).is_ok());
    // Refusals.
    let good = every_stage()[3].encode();
    let mut bad = good.clone();
    bad[4] = 2;
    assert_eq!(
        ReverseSwap::decode(&bad),
        Err(Refused::BadFollow("version"))
    );
    let mut bad = good.clone();
    bad[0] = b'X';
    assert_eq!(ReverseSwap::decode(&bad), Err(Refused::BadFollow("magic")));
    let mut bad = good.clone();
    bad[5] = 9;
    assert_eq!(ReverseSwap::decode(&bad), Err(Refused::BadFollow("stage")));
    // A flag bit that means nothing.
    let mut bad = good.clone();
    bad[5 + 1 + 32 + 4 + 8] = 4;
    assert_eq!(ReverseSwap::decode(&bad), Err(Refused::BadFollow("flags")));
    // A stage that does not agree with the phase of the state: the state of
    // a sent claim under the stage "created", and a waiting state under
    // "claim sent".
    let all = every_stage();
    let sent_bytes = all[4].encode();
    let mut bad = sent_bytes.clone();
    bad[5] = 0;
    assert_eq!(
        ReverseSwap::decode(&bad),
        Err(Refused::BadFollow("stage and phase"))
    );
    let mut bad = all[0].encode();
    bad[5] = 3;
    assert_eq!(
        ReverseSwap::decode(&bad),
        Err(Refused::BadFollow("stage and phase"))
    );
    // A lockup of less than was promised.
    let mut bad = good.clone();
    let at = 5 + 1 + 32 + 4;
    bad[at..at + 8].copy_from_slice(&1u64.to_be_bytes());
    assert_eq!(
        ReverseSwap::decode(&bad),
        Err(Refused::BadFollow("stage and phase"))
    );
    // The state inside is held to its own rules.
    let mut bad = all[0].encode();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert!(ReverseSwap::decode(&bad).is_err());
    // A failure that is not one.
    let mut bad = all[6].encode();
    bad[6] = 0;
    assert_eq!(
        ReverseSwap::decode(&bad),
        Err(Refused::BadFollow("failure"))
    );
    assert_eq!(Refused::BadFollow("x").rule(), "SW-73");
}

#[test]
fn sw_74_the_decoder_is_fuzzed() {
    // The target is in fuzz/fuzz_targets/swap_follow_decode.rs; here, a few
    // thousand changed files give a refusal or a machine that encodes back
    // to the same bytes.
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut accepted = 0;
    for m in every_stage() {
        let file = m.encode();
        for _ in 0..600 {
            let mut f = file.clone();
            for _ in 0..=(next() % 3) {
                let at = usize::try_from(next() % f.len() as u64).unwrap();
                f[at] = (next() & 0xff) as u8;
            }
            if let Ok(d) = ReverseSwap::decode(&f) {
                assert_eq!(d.encode(), f);
                accepted += 1;
            }
        }
    }
    assert!(accepted > 0);
}

// SW-46, SW-75

fn claimed_machine() -> (ReverseSwap, Transaction, crate::LockupSeen) {
    let m = machine();
    let (m, _) = m
        .on_lockup(&lockup_tx(99_350), Seen::Block, &ctx())
        .unwrap();
    let Stage::Locked(l) = m.stage() else {
        panic!()
    };
    let (m, claim) = m.claim(2_000, &ctx()).unwrap();
    (m, claim, l)
}

#[test]
fn sw_75_the_claim_seen_in_a_block_may_be_an_earlier_one_the_caller_built() {
    let (m1, first, lockup) = claimed_machine();
    let (m2, second) = m1.reclaim(&lockup, 4_000, &ctx()).unwrap();
    assert_ne!(first.compute_txid(), second.compute_txid());
    assert_eq!(
        m2.state().phase(),
        crate::Phase::Claimed(second.compute_txid())
    );
    // The first claim is the one mined: it counts when the caller names it.
    let earlier = [first.compute_txid()];
    let paid = m2
        .claim_confirmed_among(first.compute_txid(), &earlier, TIP + 1)
        .unwrap();
    assert_eq!(paid.stage(), Stage::Paid { height: TIP + 1 });
    assert_eq!(
        paid.state().phase(),
        crate::Phase::Claimed(first.compute_txid())
    );
    // Without naming it, or for a transaction of nobody's, it is refused.
    assert_eq!(
        m2.claim_confirmed(first.compute_txid(), TIP + 1)
            .unwrap_err(),
        Refused::WrongClaim
    );
    let other = Txid::from_byte_array([7; 32]);
    assert_eq!(
        m2.claim_confirmed_among(other, &earlier, TIP + 1)
            .unwrap_err(),
        Refused::WrongClaim
    );
    // The last claim needs no list.
    assert!(m2.claim_confirmed(second.compute_txid(), TIP + 1).is_ok());
}

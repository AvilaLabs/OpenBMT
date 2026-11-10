//! Signing, storing and the state that is kept (AS-33 to AS-38).

mod common;

use bitcoin::psbt::Psbt;
use common::*;
use openbmt_approver::{Choice, DecideError, FinishError, RefuseSession, RestoreError};
use openbmt_core::batch::SignedBatch;
use openbmt_core::pairing::owner_cosign;
use openbmt_core::settings::{Change, SignedSettings};
use openbmt_session::{DeclineCode, Item, Payload, Request, Response, Verdict};
use openbmt_signer::SignerState;

fn payload(r: &Response, number: u32) -> Payload {
    match &r
        .results()
        .iter()
        .find(|x| x.number == number)
        .unwrap()
        .verdict
    {
        Verdict::Approved(p) => p.clone(),
        other => panic!("not approved: {other:?}"),
    }
}

/// Key 1's signature on a file, by the real signer.
fn key1_signs(psbt: &Psbt) -> Psbt {
    let k = kit();
    let mut s = SignerState::register(
        wallet().canonical_string(),
        NET,
        &k.xpubs[0],
        Some(&k.xpubs[2]),
    )
    .unwrap();
    let review = s.review(&psbt.serialize(), NOW).unwrap();
    let signed = s
        .sign(review, &k.accounts[0], NOW, |_| {
            Ok::<(), std::convert::Infallible>(())
        })
        .unwrap();
    Psbt::deserialize(&signed.psbt).unwrap()
}

#[test]
fn as_33_an_approved_spend_is_signed_by_the_signer_and_finishes_into_a_transaction() {
    let mut a = approver_with_key2();
    let store = Store::default();
    let coins = [coin(1, 0, 300_000)];
    let psbt_bytes = psbt_bytes(&coins, vec![(external(1), 60_000)], 0);
    let request = request_with(
        1,
        vec![Item::Spend {
            psbt: psbt_bytes.clone(),
            labels: vec![refund_label(0, "Mia"), change_label(1)],
        }],
    );
    let response = approve_all(&mut a, request.as_bytes(), &store);
    let Payload::Spend { psbt } = payload(&decode(&response, &request), 0) else {
        panic!()
    };
    let by2 = Psbt::deserialize(&psbt).unwrap();
    let base = Psbt::deserialize(&psbt_bytes).unwrap();
    let by1 = key1_signs(&base);
    let joined = openbmt_spend::combine(&base, &[by1, by2]).unwrap();
    let tx = openbmt_spend::finalize(wallet(), &joined).unwrap();
    assert_eq!(tx.output[0].value.to_sat(), 60_000);
    assert_eq!(tx.output[0].script_pubkey, external(1));
    assert_eq!(tx.output.len(), 2);
    // The signer's state moved: the spend is in its log.
    assert_eq!(a.signer().unwrap().log().count(), 1);
    assert_eq!(a.signer().unwrap().change_mark(), 1);
}

#[test]
fn as_33_batches_settings_and_pairings_are_signed_through_the_core_crate() {
    let mut a = approver_with_key2();
    let store = Store::default();
    let settings = settings_with(2, &[1]);
    let (pairing, counter_side) = pairing_item(1, 2);
    let request = request_with(
        1,
        vec![
            pairing,
            settings_item(settings.clone()),
            batch_item(1, 0, 3),
            health_item(7),
        ],
    );
    let response = approve_all(&mut a, request.as_bytes(), &store);
    let r = decode(&response, &request);

    // The batch has both signatures by the core crate's own check.
    let Payload::Batch { sig } = payload(&r, 2) else {
        panic!()
    };
    let signed_batch = SignedBatch {
        batch: batch(1, 0, 3),
        owner_sig: batch(1, 0, 3).sign(&owner()),
        approver_sig: sig,
    };
    signed_batch.verify(&roots(), &policy()).unwrap();

    // The settings verify, as loosening, with both signatures.
    let Payload::Settings { sig } = payload(&r, 1) else {
        panic!()
    };
    let signed_settings = SignedSettings {
        settings: settings.clone(),
        owner_sig: settings.sign(&owner()),
        approver_sig: Some(sig),
    };
    assert_eq!(
        signed_settings.verify(&roots(), None),
        Ok(Change::Loosening)
    );

    // The pairing completes on the owner phone and on the counter.
    let Payload::Pairing {
        settings_version,
        sig,
    } = payload(&r, 0)
    else {
        panic!()
    };
    let Item::Pairing { request: pr, .. } = &request.item(0).unwrap().item else {
        panic!()
    };
    let entry = openbmt_core::pairing::PairingEntry {
        wallet: wallet_id(),
        counter: pr.counter,
        name: pr.name.clone(),
        nonce: pr.nonce,
        settings_version,
    };
    let signed = owner_cosign(&offer(), entry, sig, &owner()).unwrap();
    let paired = counter_side.finish(&signed, true, NOW + 1).unwrap();
    assert_eq!(paired.settings_floor(), 2);

    // The mark and the settings baseline moved.
    assert_eq!(a.receive_mark(), 3);
    assert_eq!(a.last_settings().unwrap().settings.version, 2);
}

#[test]
fn as_34_the_store_is_called_before_the_bytes_are_returned_with_all_new_state() {
    let mut a = approver_with_key2();
    let store = Store::default();
    let coins = [coin(1, 0, 300_000)];
    cosign_settings(&mut a, 200, 2, &[1], &store);
    let request = request_with(
        1,
        vec![
            refund_item(&coins, 1, 60_000, 0),
            batch_item(1, 0, 3),
            health_item(1),
        ],
    );
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    for n in 0..3 {
        session.decide(n, Choice::Approve).unwrap();
    }
    let calls_before = *store.calls.borrow();
    let mut stored_before_return: Option<Vec<u8>> = None;
    let response = session
        .finish(|bytes| {
            stored_before_return = Some(bytes.to_vec());
            store.put(bytes)
        })
        .unwrap();
    assert!(!response.is_empty());
    assert_eq!(*store.calls.borrow(), calls_before + 1);
    // What was stored is the whole new state: the Key app, restored from it,
    // has the log, the mark and the spend.
    let stored = stored_before_return.unwrap();
    assert_eq!(stored, a.snapshot());
    let b = restore(&stored, true).unwrap();
    assert_eq!(b.seen_sessions().last(), Some(&[1u8; 32]));
    assert_eq!(b.receive_mark(), 3);
    assert_eq!(b.signer().unwrap().log().count(), 1);
}

#[test]
fn as_34_a_failed_store_returns_nothing_and_changes_nothing() {
    let mut a = approver_with_key2();
    let store = Store::default();
    let coins = [coin(1, 0, 300_000)];
    cosign_settings(&mut a, 200, 2, &[1], &store);
    let before = a.snapshot();
    let request = request_with(
        1,
        vec![
            refund_item(&coins, 1, 60_000, 0),
            batch_item(1, 0, 3),
            health_item(1),
        ],
    );
    *store.fail.borrow_mut() = true;
    {
        let mut session = a.open(request.as_bytes(), NOW).unwrap();
        for n in 0..3 {
            session.decide(n, Choice::Approve).unwrap();
        }
        let err = session.finish(|b| store.put(b)).unwrap_err();
        assert_eq!(err, FinishError::Store("disk full".to_string()));
        // The session is still usable: the store comes back and it finishes.
        *store.fail.borrow_mut() = false;
        let response = session.finish(|b| store.put(b)).unwrap();
        decode(&response, &request);
    }
    assert_ne!(a.snapshot(), before);
    // And the failed attempt left no trace of its own: only one spend, one
    // batch and this session in the state.
    assert_eq!(a.signer().unwrap().log().count(), 1);
    assert_eq!(a.receive_mark(), 3);
}

#[test]
fn as_34_nothing_in_memory_changes_when_the_store_fails() {
    let mut a = approver_with_key2();
    let store = Store::default();
    cosign_settings(&mut a, 200, 2, &[1], &store);
    let before = a.snapshot();
    let bytes = request_bytes(1, vec![batch_item(1, 0, 3), health_item(1)]);
    *store.fail.borrow_mut() = true;
    {
        let mut session = a.open(&bytes, NOW).unwrap();
        session.decide(0, Choice::Approve).unwrap();
        session.decide(1, Choice::Approve).unwrap();
        assert!(session.finish(|b| store.put(b)).is_err());
    }
    assert_eq!(a.snapshot(), before);
    assert_eq!(a.receive_mark(), 0);
    assert_eq!(a.seen_sessions().len(), 1);
    // The same request opens again.
    assert!(a.open(&bytes, NOW).is_ok());
}

#[test]
fn as_35_a_session_answered_and_lost_cannot_be_opened_again_and_nothing_is_given_back() {
    let mut a = approver_with_key2();
    let store = Store::default();
    let coins = [coin(1, 0, 300_000)];
    cosign_settings(&mut a, 200, 2, &[1], &store);
    let first = request_with(
        1,
        vec![refund_item(&coins, 1, 60_000, 0), batch_item(1, 0, 3)],
    );
    let lost = approve_all(&mut a, first.as_bytes(), &store);
    drop(lost);
    drop(a);

    // The device restarts from what it stored.
    let stored = store.last.borrow().clone().unwrap();
    let mut a = restore(&stored, true).unwrap();
    assert_eq!(refused(&mut a, first.as_bytes()), RefuseSession::SeenBefore);
    // The ceilings the spend used are still used.
    let totals = a.signer().unwrap().totals(NOW);
    assert!(totals.other_day >= 60_000);
    // The indexes of the batch are used up: the same range is an overlap.
    assert_eq!(a.receive_mark(), 3);
    let again = request_with(2, vec![batch_item(1, 0, 3)]);
    let session = a.open(again.as_bytes(), NOW).unwrap();
    let line = &session.lines()[0];
    assert!(!line.can_approve);
    assert!(line.failure.as_ref().unwrap().contains("repeated"));
}

#[test]
fn as_35_the_same_transaction_asked_again_counts_once_and_gets_the_same_signatures() {
    let mut a = approver_with_key2();
    let store = Store::default();
    let coins = [coin(1, 0, 300_000)];
    let item = refund_item(&coins, 1, 60_000, 0);
    let first = request_with(1, vec![item.clone()]);
    let r1 = decode(&approve_all(&mut a, first.as_bytes(), &store), &first);
    let before = a.signer().unwrap().totals(NOW);
    let second = request_with(2, vec![item]);
    let r2 = decode(&approve_all(&mut a, second.as_bytes(), &store), &second);
    assert_eq!(payload(&r1, 0), payload(&r2, 0));
    assert_eq!(a.signer().unwrap().totals(NOW), before);
}

#[test]
fn as_35_a_batch_whose_indexes_were_used_up_needs_the_mark_raised_on_the_device() {
    let mut a = approver_with_key2();
    let store = Store::default();
    cosign_settings(&mut a, 200, 2, &[1], &store);
    approve_all(&mut a, &request_bytes(1, vec![batch_item(1, 0, 3)]), &store);
    // The next batch for the same counter starts at the mark.
    let bytes = request_bytes(2, vec![batch_item(1, 3, 3)]);
    assert!(a.open(&bytes, NOW).unwrap().lines()[0].can_approve);
    // A batch that skips ahead needs the owner to raise the mark on the
    // device first.
    let skip = request_bytes(3, vec![batch_item(1, 10, 3)]);
    assert!(!a.open(&skip, NOW).unwrap().lines()[0].can_approve);
    a.raise_receive_mark(10, |b| store.put(b)).unwrap();
    assert!(a.open(&skip, NOW).unwrap().lines()[0].can_approve);
}

#[test]
fn as_36_without_key_2_a_spend_is_answered_not_on_this_device_and_the_rest_works() {
    let mut a = approver_only();
    let store = Store::default();
    let coins = [coin(1, 0, 300_000)];
    let (pairing, _) = pairing_item(1, 2);
    let request = request_with(
        1,
        vec![
            refund_item(&coins, 1, 60_000, 0),
            settings_item(settings_with(2, &[1])),
            batch_item(1, 0, 3),
            pairing,
            health_item(1),
        ],
    );
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    let lines = session.lines();
    assert!(!lines[0].can_approve);
    assert!(lines[0]
        .failure
        .as_ref()
        .unwrap()
        .contains("not on this device"));
    assert!(matches!(
        session.decide(0, Choice::Approve),
        Err(DecideError::CannotApprove(_))
    ));
    for n in 1..5 {
        session.decide(n, Choice::Approve).unwrap();
    }
    let response = session.finish(|b| store.put(b)).unwrap();
    let r = decode(&response, &request);
    assert!(matches!(
        &r.results()[0].verdict,
        Verdict::Declined {
            code: DeclineCode::NotOnThisDevice,
            ..
        }
    ));
    for n in 1..5 {
        assert!(
            matches!(r.results()[n].verdict, Verdict::Approved(_)),
            "{n}"
        );
    }
    assert_eq!(a.receive_mark(), 3);
}

#[test]
fn as_36_a_spend_declined_by_the_person_is_still_answered_not_on_this_device() {
    let mut a = approver_only();
    let store = Store::default();
    let coins = [coin(1, 0, 300_000)];
    let request = request_with(1, vec![refund_item(&coins, 1, 60_000, 0)]);
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    session.decide(0, Choice::Decline("no".into())).unwrap();
    let response = session.finish(|b| store.put(b)).unwrap();
    assert!(matches!(
        &decode(&response, &request).results()[0].verdict,
        Verdict::Declined {
            code: DeclineCode::NotOnThisDevice,
            ..
        }
    ));
}

#[test]
fn as_37_a_response_above_8_mib_is_an_error_before_anything_is_stored() {
    // The signer adds a signature to each input, so a request just under the
    // limit gives a response above it.
    let mut a = approver_with_key2_ceilings(Some(openbmt_signer::Ceilings {
        per_tx: openbmt_signer::MAX_MONEY_SATS,
        per_day: openbmt_signer::MAX_MONEY_SATS,
        per_week: openbmt_signer::MAX_MONEY_SATS,
    }));
    let store = Store::default();
    let (request, _) = near_limit_request();
    assert!(request.as_bytes().len() <= openbmt_session::MAX_SESSION_BYTES);
    let before = a.snapshot();
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    session.decide(0, Choice::Approve).unwrap();
    let err = session.finish(|b| store.put(b)).unwrap_err();
    assert!(matches!(err, FinishError::Response(_)), "{err:?}");
    assert_eq!(*store.calls.borrow(), 0);
    drop(session);
    assert_eq!(a.snapshot(), before);
}

/// A request with one spend of 900 inputs whose file is just under 8 MiB.
fn near_limit_request() -> (Request, usize) {
    use bitcoin::absolute::LockTime;
    use bitcoin::hashes::Hash;
    use bitcoin::transaction::Version;
    use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, Witness};
    let limit = openbmt_session::MAX_SESSION_BYTES;
    let inputs = 900usize;
    let script = wallet().receive_script(0).unwrap();
    let build = |per_tx: usize| -> Request {
        let tx_count = inputs.div_ceil(per_tx);
        let mut coins = Vec::new();
        for t in 0..tx_count {
            let mut seed = [0u8; 32];
            seed[..8].copy_from_slice(&(t as u64 + 1).to_le_bytes());
            let prev = Transaction {
                version: Version::TWO,
                lock_time: LockTime::ZERO,
                input: vec![TxIn {
                    previous_output: OutPoint {
                        txid: Txid::from_byte_array(seed),
                        vout: 0,
                    },
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::MAX,
                    witness: Witness::new(),
                }],
                output: (0..per_tx)
                    .map(|_| TxOut {
                        value: Amount::from_sat(1_000),
                        script_pubkey: script.clone(),
                    })
                    .collect(),
            };
            let txid = prev.compute_txid();
            for v in 0..per_tx {
                if coins.len() == inputs {
                    break;
                }
                coins.push(openbmt_spend::Coin {
                    outpoint: OutPoint {
                        txid,
                        vout: v as u32,
                    },
                    prev_tx: prev.clone(),
                    vout: v as u32,
                    chain: openbmt_core::wallet::Keychain::Receive,
                    index: 0,
                    confirmations: 6,
                });
            }
        }
        let mut req = spend_request(vec![(external(1), 0)], 0);
        req.send_all = true;
        let plan = openbmt_spend::plan(wallet(), &coins, &req).unwrap();
        let psbt = plan.to_psbt(wallet()).unwrap().serialize();
        request_with(
            1,
            vec![Item::Spend {
                psbt,
                labels: vec![openbmt_session::OutputLabel {
                    output: 0,
                    label: openbmt_session::Label::CashOut {
                        destination: "everything".into(),
                    },
                }],
            }],
        )
    };
    // Each prior transaction is about 43 bytes an output and sits in the file
    // once for every input that spends it. Find the size that leaves room
    // for the signatures only.
    let mut per_tx = 195usize;
    loop {
        let request = build(per_tx);
        let len = request.as_bytes().len();
        if len > limit {
            per_tx -= 1;
        } else if len + 90_000 < limit {
            per_tx += 1;
        } else {
            return (request, per_tx);
        }
    }
}

#[test]
fn as_38_the_state_round_trips_and_is_refused_when_it_does_not_fit() {
    let mut a = approver_with_key2();
    let store = Store::default();
    let coins = [coin(1, 0, 300_000)];
    cosign_settings(&mut a, 200, 2, &[1], &store);
    approve_all(
        &mut a,
        &request_bytes(
            1,
            vec![refund_item(&coins, 1, 60_000, 0), batch_item(1, 0, 3)],
        ),
        &store,
    );
    let bytes = a.snapshot();
    assert_eq!(&bytes[..5], b"OBM3\x01");
    let b = restore(&bytes, true).unwrap();
    assert_eq!(b.snapshot(), bytes);
    assert_eq!(b.seen_sessions(), a.seen_sessions());
    assert_eq!(b.receive_mark(), 3);
    // Cut and extended files.
    assert!(matches!(
        restore(&bytes[..bytes.len() - 1], true),
        Err(RestoreError::Decode(_))
    ));
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(matches!(
        restore(&longer, true),
        Err(RestoreError::Decode(_))
    ));
    for cut in [0, 4, 5, 40, 100] {
        assert!(restore(&bytes[..cut], true).is_err());
    }
    // Key 2 is on this device or it is not.
    assert!(matches!(
        restore(&bytes, false),
        Err(RestoreError::SignerMismatch)
    ));
    let only = approver_only().snapshot();
    assert!(matches!(
        restore(&only, true),
        Err(RestoreError::SignerMismatch)
    ));
    assert!(restore(&only, false).is_ok());
}

#[test]
fn as_38_a_snapshot_for_another_wallet_or_owner_is_refused() {
    let a = approver_only();
    let mut bytes = a.snapshot();
    bytes[5] ^= 1;
    assert!(matches!(
        restore(&bytes, false),
        Err(RestoreError::WrongWallet)
    ));
    let mut bytes = a.snapshot();
    bytes[5 + 32] ^= 1;
    assert!(matches!(
        restore(&bytes, false),
        Err(RestoreError::WrongOwner)
    ));
}

#[test]
fn as_38_stored_settings_must_carry_this_key_apps_signature() {
    let mut a = approver_only();
    let store = Store::default();
    cosign_settings(&mut a, 200, 2, &[1], &store);
    let bytes = a.snapshot();
    assert!(restore(&bytes, false).is_ok());
    // Replace the approver signature in the stored settings by zeros: the
    // settings are the second to last field, before the log.
    let mut bad = bytes.clone();
    let log_len = 2 + 32;
    let end = bad.len() - log_len;
    for b in &mut bad[end - 64..end] {
        *b = 0;
    }
    assert!(matches!(
        restore(&bad, false),
        Err(RestoreError::BadSettings)
    ));
}

#[test]
fn as_38_an_own_screen_call_that_changes_nothing_stores_nothing() {
    let mut a = approver_with_key2();
    let saves = std::cell::Cell::new(0u32);
    let persist = |_: &[u8]| {
        saves.set(saves.get() + 1);
        Ok::<(), ()>(())
    };
    // The clock is not behind the latest time seen: nothing to correct.
    a.change_signer(persist, |s| {
        s.correct_clock(1_000, |_| Ok::<(), std::convert::Infallible>(()))
            .map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(saves.get(), 0);
    // A real change is stored once.
    a.change_signer(persist, |s| {
        s.raise_change_mark(7, |_| Ok::<(), std::convert::Infallible>(()))
            .map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(saves.get(), 1);
    // And the same clock call again is still a no-op.
    a.change_signer(persist, |s| {
        s.correct_clock(1_000, |_| Ok::<(), std::convert::Infallible>(()))
            .map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(saves.get(), 1);
    assert_eq!(a.signer().unwrap().change_mark(), 7);
}

#[test]
fn as_38_the_own_screen_calls_are_stored_with_the_rest() {
    let mut a = approver_with_key2();
    let store = Store::default();
    a.change_signer(
        |b| store.put(b),
        |s| {
            s.set_ceilings(
                openbmt_signer::FIRST_OTHER,
                openbmt_signer::FIRST_ALLOWLISTED,
                |_| Ok::<(), std::convert::Infallible>(()),
            )
            .map_err(|e| e.to_string())?;
            s.raise_change_mark(5, |_| Ok::<(), std::convert::Infallible>(()))
                .map_err(|e| e.to_string())
        },
    )
    .unwrap();
    assert_eq!(a.signer().unwrap().change_mark(), 5);
    let stored = store.last.borrow().clone().unwrap();
    assert_eq!(
        restore(&stored, true)
            .unwrap()
            .signer()
            .unwrap()
            .change_mark(),
        5
    );
    // A failed store changes nothing.
    *store.fail.borrow_mut() = true;
    assert!(a
        .change_signer(
            |b| store.put(b),
            |s| s
                .raise_change_mark(9, |_| Ok::<(), std::convert::Infallible>(()))
                .map_err(|e| e.to_string())
        )
        .is_err());
    assert_eq!(a.signer().unwrap().change_mark(), 5);
}

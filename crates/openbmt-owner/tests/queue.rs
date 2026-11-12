//! The queue: bounds, destinations, amounts, batches, settings and the
//! stored snapshot (AS-50 to AS-52, AS-54, AS-58, AS-59, AS-65).

mod common;

use bitcoin::ScriptBuf;
use common::*;
use openbmt_owner::{
    CashAmount, Delivery, Options, Owner, RateNote, RefundBooks, Refusal, RestoreError, SaleFacts,
    SettingsQueued, Stage,
};

// ---- AS-50 -----------------------------------------------------------------------------

#[test]
fn as_50_the_bounds_are_the_documented_ones() {
    assert_eq!(openbmt_owner::MAX_REFUNDS, 256);
    assert_eq!(openbmt_owner::MAX_CASH_OUTS, 16);
    assert_eq!(openbmt_owner::MAX_BATCHES, 16);
    assert_eq!(openbmt_owner::MAX_SETTINGS, 16);
    assert_eq!(openbmt_owner::MAX_PAIRINGS, 16);
    assert_eq!(openbmt_owner::MAX_SALES, 1024);
    assert_eq!(openbmt_owner::MAX_OPEN_SESSIONS, 8);
    assert_eq!(openbmt_owner::MAX_OUTBOX, 256);
    assert_eq!(openbmt_owner::MAX_REFUNDS_PER_TX, 99);
    assert_eq!(openbmt_owner::MAX_TXS_PER_SESSION, 16);
    assert_eq!(openbmt_owner::MAX_ITEMS_PER_SESSION, 64);
}

#[test]
fn as_50_the_refund_queue_holds_256() {
    let mut o = new_owner(Options::default());
    set_facts(&mut o, sale(1), deep(1_000_000_000));
    for _ in 0..256 {
        ok(o.queue_refund(sale(1), "Mia", external(1), 1_000, None, nop));
    }
    let before = o.snapshot().unwrap();
    assert_eq!(
        refused(o.queue_refund(sale(1), "Mia", external(1), 1_000, None, nop)),
        Refusal::Full("the refund queue")
    );
    assert_eq!(o.snapshot().unwrap(), before);
}

#[test]
fn as_50_the_cash_out_batch_and_pairing_queues_are_bounded() {
    let mut o = new_owner(Options::default());
    for _ in 0..16 {
        ok(o.queue_cash_out(external(9), CashAmount::All, "Bank", nop));
    }
    assert!(matches!(
        refused(o.queue_cash_out(external(9), CashAmount::All, "Bank", nop)),
        Refusal::Full(_)
    ));
    for i in 0..16 {
        ok(o.queue_batch(batch(1, i * 3, 3), 1_000, nop));
    }
    assert!(matches!(
        refused(o.queue_batch(batch(1, 48, 3), 1_000, nop)),
        Refusal::Full(_)
    ));
    for n in 1..=16u8 {
        let (_, request) = pair_request(&o, n);
        ok(o.queue_pairing(&request, nop));
    }
    let (_, request) = pair_request(&o, 17);
    assert!(matches!(
        refused(o.queue_pairing(&request, nop)),
        Refusal::Full(_)
    ));
}

#[test]
fn as_50_the_list_of_sales_is_bounded() {
    let mut o = new_owner(Options::default());
    for n in 0..1024 {
        set_facts(&mut o, sale(n), deep(1));
    }
    let err = o
        .set_sale_facts(sale(5000), deep(1), nop)
        .expect_err("refused");
    assert!(matches!(
        err,
        openbmt_owner::OwnerError::Refused(Refusal::Full(_))
    ));
    // A sale that is already kept can still be updated.
    set_facts(&mut o, sale(0), deep(2));
}

#[test]
fn as_50_open_sessions_are_bounded() {
    let mut rig = Rig::new();
    for _ in 0..8 {
        rig.build(&[], 0);
    }
    let random = rig.random();
    let err = rig
        .owner
        .build_request(&[], 1_000, 0, 0, NOW, &random, nop)
        .expect_err("refused");
    assert!(matches!(
        err,
        openbmt_owner::OwnerError::Refused(Refusal::Full(_))
    ));
}

#[test]
fn as_50_finished_objects_waiting_are_bounded() {
    let mut rig = Rig::new();
    ok(rig
        .owner
        .queue_settings(settings_with(1, &[1]), |b| rig.owner_store.put(b)));
    rig.round(&[], 0);
    let first = rig.owner.outbox()[0].id;
    ok(rig.owner.mark_delivered(first, nop));
    // Tightening settings are finished at once and wait in the outbox.
    for version in 2..2 + 256 {
        let out = ok(rig.owner.queue_settings(settings_with(version, &[1]), nop));
        assert!(matches!(out, SettingsQueued::Tightening { .. }));
    }
    assert_eq!(rig.owner.outbox().len(), 256);
    let err = rig
        .owner
        .queue_settings(settings_with(300, &[1]), nop)
        .expect_err("refused");
    assert!(matches!(
        err,
        openbmt_owner::OwnerError::Refused(Refusal::OutboxFull)
    ));
}

#[test]
fn as_50_a_failed_store_changes_nothing() {
    let mut o = new_owner(Options::default());
    set_facts(&mut o, sale(1), deep(10_000));
    let before = o.snapshot().unwrap();
    let store = Store::default();
    store.fail_next(true);
    let err = o
        .queue_refund(sale(1), "Mia", external(1), 1_000, None, |b| store.put(b))
        .expect_err("fails");
    assert_eq!(
        err,
        openbmt_owner::OwnerError::Store("disk full".to_string())
    );
    assert_eq!(o.snapshot().unwrap(), before);
    assert!(o.refunds().is_empty());
    store.fail_next(false);
    ok(o.queue_refund(sale(1), "Mia", external(1), 1_000, None, |b| store.put(b)));
    assert_eq!(o.refunds().len(), 1);
}

// ---- AS-51 -----------------------------------------------------------------------------

fn dust_case(script: ScriptBuf, dust: u64) {
    let mut o = new_owner(Options::default());
    set_facts(&mut o, sale(1), deep(1_000_000));
    assert_eq!(
        refused(o.queue_refund(sale(1), "Mia", script.clone(), dust - 1, None, nop)),
        Refusal::BelowDust {
            sats: dust - 1,
            dust
        }
    );
    ok(o.queue_refund(sale(1), "Mia", script.clone(), dust, None, nop));
    assert_eq!(
        refused(o.queue_cash_out(script.clone(), CashAmount::Sats(dust - 1), "Bank", nop)),
        Refusal::BelowDust {
            sats: dust - 1,
            dust
        }
    );
    ok(o.queue_cash_out(script, CashAmount::Sats(dust), "Bank", nop));
}

#[test]
fn as_51_each_kind_has_its_own_dust_limit() {
    use bitcoin::hashes::Hash;
    use bitcoin::{PubkeyHash, ScriptHash, WPubkeyHash, WScriptHash};
    dust_case(
        ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([1; 20])),
        294,
    );
    dust_case(
        ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([1; 32])),
        330,
    );
    dust_case(
        ScriptBuf::new_p2pkh(&PubkeyHash::from_byte_array([1; 20])),
        546,
    );
    dust_case(
        ScriptBuf::new_p2sh(&ScriptHash::from_byte_array([1; 20])),
        540,
    );
}

#[test]
fn as_51_other_scripts_and_huge_amounts_are_refused() {
    let mut o = new_owner(Options::default());
    set_facts(&mut o, sale(1), deep(u64::MAX));
    let op_return = ScriptBuf::new_op_return([1u8; 4]);
    assert_eq!(
        refused(o.queue_refund(sale(1), "Mia", op_return.clone(), 10_000, None, nop)),
        Refusal::DestinationKind
    );
    assert_eq!(
        refused(o.queue_cash_out(op_return.clone(), CashAmount::All, "Bank", nop)),
        Refusal::DestinationKind
    );
    assert_eq!(
        refused(o.queue_refund(
            sale(1),
            "Mia",
            external(1),
            2_100_000_000_000_001,
            None,
            nop
        )),
        Refusal::AmountTooLarge
    );
    assert!(matches!(
        refused(o.queue_refund(sale(1), "Mia", external(1), 0, None, nop)),
        Refusal::BelowDust { .. }
    ));
}

#[test]
fn as_51_texts_follow_the_label_rule() {
    let mut o = new_owner(Options::default());
    set_facts(&mut o, sale(1), deep(1_000_000));
    for bad in ["", "a\nb", &"x".repeat(65)] {
        assert_eq!(
            refused(o.queue_refund(sale(1), bad, external(1), 1_000, None, nop)),
            Refusal::BadText("cashier")
        );
        assert_eq!(
            refused(o.queue_cash_out(external(1), CashAmount::All, bad, nop)),
            Refusal::BadText("cash-out label")
        );
    }
}

// ---- AS-52 -----------------------------------------------------------------------------

#[test]
fn as_52_a_sale_without_facts_takes_no_refund() {
    let mut o = new_owner(Options::default());
    assert_eq!(
        refused(o.queue_refund(sale(1), "Mia", external(1), 1_000, None, nop)),
        Refusal::UnknownSale
    );
}

#[test]
fn as_52_the_refunds_of_a_sale_add_up_to_at_most_what_was_received() {
    let mut o = new_owner(Options::default());
    set_facts(&mut o, sale(1), deep(3_000));
    let a = ok(o.queue_refund(sale(1), "Mia", external(1), 1_000, None, nop));
    ok(o.queue_refund(sale(1), "Mia", external(2), 1_500, None, nop));
    assert_eq!(
        refused(o.queue_refund(sale(1), "Mia", external(3), 501, None, nop)),
        Refusal::MoreThanReceived {
            received: 3_000,
            wanted: 3_001
        }
    );
    assert_eq!(o.refunded_sats(&sale(1)), 2_500);
    // Another sale is counted on its own.
    set_facts(&mut o, sale(2), deep(600));
    ok(o.queue_refund(sale(2), "Mia", external(3), 600, None, nop));
    // Cancelling frees the amount.
    ok(o.cancel(a, nop));
    assert_eq!(o.refunded_sats(&sale(1)), 1_500);
    ok(o.queue_refund(sale(1), "Mia", external(3), 1_500, None, nop));
}

#[test]
fn as_52_facts_that_count_refunds_cannot_be_forgotten() {
    let mut o = new_owner(Options::default());
    set_facts(&mut o, sale(1), deep(3_000));
    let id = ok(o.queue_refund(sale(1), "Mia", external(1), 1_000, None, nop));
    assert_eq!(refused(o.forget_sale(&sale(1), nop)), Refusal::SaleInUse);
    ok(o.cancel(id, nop));
    ok(o.forget_sale(&sale(1), nop));
    assert!(o.sale_facts(&sale(1)).is_none());
}

// ---- AS-54 -----------------------------------------------------------------------------

#[test]
fn as_54_cancel_and_requeue_follow_the_stages() {
    let mut o = new_owner(Options::default());
    set_facts(&mut o, sale(1), deep(10_000));
    let id = ok(o.queue_refund(sale(1), "Mia", external(1), 1_000, None, nop));
    assert_eq!(refused(o.cancel(99, nop)), Refusal::UnknownEntry(99));
    assert!(matches!(
        refused(o.requeue(id, nop)),
        Refusal::WrongStage(_)
    ));
    ok(o.cancel(id, nop));
    assert!(o.refunds().is_empty());
}

// ---- AS-58 -----------------------------------------------------------------------------

#[test]
fn as_58_a_batch_must_be_what_the_wallet_derives() {
    let mut o = new_owner(Options::default());
    let mut b = batch(1, 0, 3);
    b.scripts.swap(0, 1);
    assert_eq!(
        refused(o.queue_batch(b, 1_000, nop)),
        Refusal::BatchNotDerived
    );
    // Fewer than the policy minimum is not derived either.
    assert_eq!(
        refused(o.queue_batch(batch(1, 0, 3).clone_with_count(2), 1_000, nop)),
        Refusal::BatchNotDerived
    );
    let id = ok(o.queue_batch(batch(1, 0, 3), 1_000, nop));
    assert_eq!(o.batches().len(), 1);
    assert_eq!(o.batches().first().unwrap().id, id);
}

trait CloneWithCount {
    fn clone_with_count(&self, count: u32) -> Self;
}

impl CloneWithCount for openbmt_core::batch::AddressBatch {
    fn clone_with_count(&self, count: u32) -> Self {
        let mut b = self.clone();
        b.count = count;
        b.scripts.truncate(count as usize);
        b
    }
}

#[test]
fn as_58_a_pairing_request_is_decoded_strictly_and_checked() {
    let mut o = new_owner(Options::default());
    assert!(matches!(
        refused(o.queue_pairing(b"junk", nop)),
        Refusal::PairingNotRequest(_)
    ));
    let (_, mut request) = pair_request(&o, 1);
    request.push(0);
    assert!(matches!(
        refused(o.queue_pairing(&request, nop)),
        Refusal::PairingNotRequest(_)
    ));
    // A request made for another offer does not fit this owner and Key app.
    let other = new_owner_with_approver(identity([0x55; 32]).device_id());
    let (_, request) = pair_request(&other, 1);
    assert_eq!(
        refused(o.queue_pairing(&request, nop)),
        Refusal::PairingOtherShop
    );
    let (_, request) = pair_request(&o, 1);
    let id = ok(o.queue_pairing(&request, nop));
    assert_eq!(queued_pairing_of(&o, id).stage, Stage::Queued);
}

fn new_owner_with_approver(approver: openbmt_core::identity::DeviceId) -> Owner {
    Owner::new(
        &OWNER_SECRET,
        wallet().clone(),
        approver,
        policy(),
        Options::default(),
    )
    .unwrap()
}

// ---- AS-59 -----------------------------------------------------------------------------

#[test]
fn as_59_settings_are_validated_and_must_rise_in_version() {
    let mut o = new_owner(Options::default());
    let mut bad = settings_with(1, &[1]);
    bad.min_confirmations = 0;
    assert!(matches!(
        refused(o.queue_settings(bad, nop)),
        Refusal::SettingsInvalid(_)
    ));
    ok(o.queue_settings(settings_with(5, &[1]), nop));
    assert_eq!(
        refused(o.queue_settings(settings_with(5, &[1]), nop)),
        Refusal::SettingsVersion { known: 5, got: 5 }
    );
    assert_eq!(
        refused(o.queue_settings(settings_with(4, &[1]), nop)),
        Refusal::SettingsVersion { known: 5, got: 4 }
    );
}

#[test]
fn as_59_the_first_settings_loosen_and_are_queued() {
    let mut o = new_owner(Options::default());
    let out = ok(o.queue_settings(settings_with(1, &[1]), nop));
    assert!(matches!(out, SettingsQueued::Queued(_)));
    assert_eq!(o.queued_settings().len(), 1);
    assert!(o.known_settings().is_none());
}

#[test]
fn as_59_a_tightening_change_is_returned_at_once_and_supersedes_the_queued() {
    let mut rig = Rig::new();
    ok(rig.owner.queue_settings(settings_with(1, &[1]), nop));
    rig.round(&[], 0);
    assert_eq!(rig.owner.known_settings().unwrap().version, 1);
    // A loosening change (a second counter) is queued.
    let queued = ok(rig.owner.queue_settings(settings_with(2, &[1, 2]), nop));
    let SettingsQueued::Queued(queued_id) = queued else {
        panic!("{queued:?}");
    };
    // A change that only tightens is signed by the owner alone.
    let mut tighter = settings_with(3, &[1]);
    tighter.version = 3;
    let out = ok(rig.owner.queue_settings(tighter.clone(), nop));
    let SettingsQueued::Tightening { id, signed } = out else {
        panic!("{out:?}");
    };
    assert!(signed.approver_sig.is_none());
    assert_eq!(signed.settings, tighter);
    // It is also waiting among the finished objects, and now in force.
    assert!(matches!(
        rig.owner.outbox().iter().find(|d| d.id == id).unwrap().what,
        Delivery::Settings(_)
    ));
    assert_eq!(rig.owner.known_settings().unwrap().version, 3);
    // The queued loosening of a lower version was dropped as superseded.
    assert!(rig
        .owner
        .queued_settings()
        .iter()
        .all(|q| q.id != queued_id));
    assert!(rig.owner.queued_settings().is_empty());
}

// ---- AS-65 -----------------------------------------------------------------------------

/// An owner with something of everything, and a session open.
fn rich(rig: &mut Rig) {
    let o = &mut rig.owner;
    ok(o.queue_settings(settings_with(1, &[1]), nop));
    set_facts(o, sale(1), deep(500_000));
    ok(o.queue_refund(sale(1), "Mia", external(1), 40_000, None, nop));
    ok(o.queue_cash_out(external(9), CashAmount::All, "Bank", nop));
    ok(o.queue_batch(batch(1, 0, 3), 1_000, nop));
    let (_, request) = pair_request(o, 1);
    ok(o.queue_pairing(&request, nop));
}

#[test]
fn as_65_a_snapshot_restores_to_the_same_snapshot() {
    let mut rig = Rig::new();
    rich(&mut rig);
    rig.build(&[coin(1, 0, 300_000), coin(2, 1, 300_000)], 0);
    // Something queued after the session, and something finished.
    set_facts(&mut rig.owner, sale(2), deep(9_000));
    ok(rig
        .owner
        .queue_refund(sale(2), "Zed", external(2), 3_000, None, nop));
    let bytes = rig.owner.snapshot().unwrap();
    assert_eq!(&bytes[..5], b"OBM4\x01");
    let again = restore_owner(&bytes).unwrap();
    assert_eq!(again.snapshot().unwrap(), bytes);
    assert_eq!(again.refunds(), rig.owner.refunds());
    assert_eq!(again.open_sessions(), rig.owner.open_sessions());
}

#[test]
fn as_65_a_snapshot_with_finished_objects_restores() {
    let mut rig = Rig::new();
    rich(&mut rig);
    let taken = rig.round(&[coin(1, 0, 300_000), coin(2, 1, 300_000)], 0);
    assert!(!taken.outcomes.is_empty());
    assert!(!rig.owner.outbox().is_empty());
    let bytes = rig.owner.snapshot().unwrap();
    let again = restore_owner(&bytes).unwrap();
    assert_eq!(again.snapshot().unwrap(), bytes);
    assert_eq!(again.outbox(), rig.owner.outbox());
    assert_eq!(again.known_settings(), rig.owner.known_settings());
}

#[test]
fn as_65_a_snapshot_for_another_owner_or_key_app_is_refused() {
    let mut rig = Rig::new();
    rich(&mut rig);
    let bytes = rig.owner.snapshot().unwrap();
    let err = Owner::restore(
        &[0x12; 32],
        wallet().clone(),
        approver_id(),
        policy(),
        &bytes,
    )
    .err()
    .unwrap();
    assert_eq!(err, RestoreError::WrongOwner);
    let err = Owner::restore(
        &OWNER_SECRET,
        wallet().clone(),
        identity([0x55; 32]).device_id(),
        policy(),
        &bytes,
    )
    .err()
    .unwrap();
    assert_eq!(err, RestoreError::WrongApprover);
    let other_wallet = other_wallet();
    let err = Owner::restore(&OWNER_SECRET, other_wallet, approver_id(), policy(), &bytes)
        .err()
        .unwrap();
    assert_eq!(err, RestoreError::WrongWallet);
}

fn other_wallet() -> openbmt_core::wallet::WalletDescription {
    use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
    use openbmt_core::wallet::{KeySlot, WalletDescription};
    let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
    let mut slots = Vec::new();
    for n in 0..3u8 {
        let master = Xpriv::new_master(NET, &[n + 40; 32]).unwrap();
        let account = master.derive_priv(secp(), &path).unwrap();
        slots.push(KeySlot {
            index: n + 1,
            origin_fingerprint: master.fingerprint(secp()),
            origin_path: path.clone(),
            xpub: Xpub::from_priv(secp(), &account),
        });
    }
    WalletDescription::new(NET, slots.try_into().unwrap()).unwrap()
}

#[test]
fn as_65_every_cut_is_refused() {
    let mut rig = Rig::new();
    rich(&mut rig);
    rig.build(&[coin(1, 0, 300_000)], 0);
    let bytes = rig.owner.snapshot().unwrap();
    // Every cut up to the end of the fixed head, then a stride, so that
    // the check stays fast; the last byte is always cut.
    let mut cuts: Vec<usize> = (0..200).collect();
    cuts.extend((200..bytes.len()).step_by(13));
    cuts.push(bytes.len() - 1);
    for cut in cuts {
        assert!(restore_owner(&bytes[..cut]).is_err(), "cut at {cut}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(restore_owner(&longer).is_err());
}

#[test]
fn as_65_a_changed_byte_is_refused_or_is_a_snapshot_of_what_it_says() {
    let mut rig = Rig::new();
    rich(&mut rig);
    rig.build(&[coin(1, 0, 300_000)], 0);
    let bytes = rig.owner.snapshot().unwrap();
    let mut accepted = 0;
    for at in (0..bytes.len()).step_by(11) {
        let mut b = bytes.clone();
        b[at] ^= 0x01;
        match restore_owner(&b) {
            Err(_) => {}
            // Whatever is accepted must be exactly what encoding it gives.
            Ok(o) => {
                accepted += 1;
                assert_eq!(o.snapshot().unwrap(), b, "byte {at}");
            }
        }
    }
    // Most bytes are covered by a signature, a derivation or a rule.
    assert!(accepted < bytes.len() / 11);
}

#[test]
fn as_65_arbitrary_bytes_never_panic() {
    // A fixed-seed generator: the same inputs on every run.
    let mut x: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..300 {
        let len = (next() % 400) as usize;
        let mut bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        // Half of them start like a snapshot, so the parser gets further.
        if next() % 2 == 0 && bytes.len() >= 5 {
            bytes[..5].copy_from_slice(b"OBM4\x01");
        }
        let _ = restore_owner(&bytes);
    }
}

#[test]
fn as_65_a_frozen_item_that_is_not_in_its_request_is_refused() {
    let mut rig = Rig::new();
    set_facts(&mut rig.owner, sale(1), deep(500_000));
    ok(rig
        .owner
        .queue_refund(sale(1), "Mia", external(1), 40_000, None, nop));
    rig.build(&[coin(1, 0, 300_000)], 0);
    let bytes = rig.owner.snapshot().unwrap();
    // Raise the frozen payment's amount by one satoshi. The amount sits
    // after the script; find the 8 bytes of 40,000 and bump the first.
    let needle = 40_000u64.to_le_bytes();
    let at = bytes
        .windows(8)
        .position(|w| w == needle)
        .expect("the frozen amount is in the snapshot");
    // The first match is the refund's own amount; the frozen one comes
    // later in the snapshot.
    let later = bytes[at + 8..]
        .windows(8)
        .position(|w| w == needle)
        .map(|p| p + at + 8)
        .expect("a second copy");
    let mut b = bytes.clone();
    b[later] ^= 1;
    assert!(restore_owner(&b).is_err());
}

#[test]
fn as_65_a_frozen_change_position_that_is_not_the_requests_is_refused() {
    let mut rig = Rig::new();
    set_facts(&mut rig.owner, sale(1), deep(500_000));
    ok(rig
        .owner
        .queue_refund(sale(1), "Mia", external(1), 40_000, None, nop));
    let built = rig.build(&[coin(1, 0, 300_000)], 0);
    let request = openbmt_session::Request::from_bytes(&built.request).unwrap();
    let openbmt_session::Item::Spend { psbt, .. } = &request.items()[0].item else {
        panic!()
    };
    let tx = bitcoin::psbt::Psbt::deserialize(psbt).unwrap().unsigned_tx;
    let place = tx
        .output
        .iter()
        .position(|o| o.script_pubkey == wallet().change_script(0).unwrap())
        .unwrap();
    let bytes = rig.owner.snapshot().unwrap();
    assert!(restore_owner(&bytes).is_ok());
    // The frozen payment is the last place the amount appears (the open
    // sessions follow the queue); after it come the change index (4 bytes),
    // a flag and the change position (4 bytes).
    let needle = 40_000u64.to_le_bytes();
    let frozen = bytes
        .windows(8)
        .rposition(|w| w == needle)
        .expect("the frozen amount is in the snapshot");
    let flag = frozen + 8 + 4;
    assert_eq!(bytes[flag], 1, "the change is frozen");
    assert_eq!(bytes[flag + 1..flag + 5], (place as u32).to_le_bytes());
    // The other place, a place past the outputs, and no change at all.
    for other in [1 - place as u32, 2, u32::MAX] {
        let mut b = bytes.clone();
        b[flag + 1..flag + 5].copy_from_slice(&other.to_le_bytes());
        assert!(restore_owner(&b).is_err(), "place {other}");
    }
    let mut b = bytes.clone();
    b[flag] = 0;
    assert!(restore_owner(&b).is_err(), "no change frozen");
}

#[test]
fn as_65_a_queued_batch_without_the_owners_signature_is_refused() {
    let mut o = new_owner(Options::default());
    ok(o.queue_batch(batch(1, 0, 3), 1_000, nop));
    let bytes = o.snapshot().unwrap();
    let sig = o.batches()[0].owner_sig.0;
    let at = bytes
        .windows(64)
        .position(|w| w == sig)
        .expect("the signature is in the snapshot");
    let mut b = bytes.clone();
    b[at + 10] ^= 1;
    assert!(matches!(
        restore_owner(&b),
        Err(RestoreError::Inconsistent(_))
    ));
    assert!(restore_owner(&bytes).is_ok());
}

#[test]
fn as_65_a_sale_facts_value_survives() {
    let mut o = new_owner(Options::default());
    set_facts(&mut o, sale(7), shallow(123));
    let again = restore_owner(&o.snapshot().unwrap()).unwrap();
    assert_eq!(
        again.sale_facts(&sale(7)),
        Some(SaleFacts {
            received_sats: 123,
            reached_refund_depth: false
        })
    );
}

// ---- OC-51 -----------------------------------------------------------------------------

fn rate(minor_per_btc: u64) -> RateNote {
    RateNote {
        currency: *b"USD",
        minor_per_btc,
    }
}

#[test]
fn oc_51_a_price_and_the_books_notes_are_stored_in_version_3_and_restore() {
    let mut o = new_owner(Options::default());
    let plain = o.snapshot().unwrap();
    assert_eq!(plain[4], 1);
    set_facts(&mut o, sale(1), deep(1_000_000));
    let r = ok(o.queue_refund_with_books(
        sale(1),
        "Mia",
        external(1),
        1_000,
        None,
        Some(RefundBooks {
            request: 9,
            last_quote: Some(rate(9_500_000)),
        }),
        nop,
    ));
    let bare = ok(o.queue_refund(sale(1), "Mia", external(2), 1_000, None, nop));
    let c = ok(o.queue_cash_out_priced(
        external(3),
        CashAmount::All,
        "Bank",
        Some(rate(9_400_000)),
        nop,
    ));
    let plain_cash = ok(o.queue_cash_out(external(4), CashAmount::All, "Bank", nop));
    let bytes = o.snapshot().unwrap();
    assert_eq!(bytes[4], 3);
    let back = restore_owner(&bytes).unwrap();
    assert_eq!(back.snapshot().unwrap(), bytes);
    let refund = |id| back.refunds().iter().find(|x| x.id == id).unwrap().books;
    assert_eq!(
        refund(r),
        Some(RefundBooks {
            request: 9,
            last_quote: Some(rate(9_500_000))
        })
    );
    assert_eq!(refund(bare), None);
    let price = |id| back.cash_outs().iter().find(|x| x.id == id).unwrap().price;
    assert_eq!(price(c), Some(rate(9_400_000)));
    assert_eq!(price(plain_cash), None);
}

#[test]
fn oc_51_a_price_that_is_zero_or_not_a_currency_is_refused_and_a_damaged_list_is_refused() {
    let mut o = new_owner(Options::default());
    let zero = RateNote {
        currency: *b"USD",
        minor_per_btc: 0,
    };
    assert!(matches!(
        refused(o.queue_cash_out_priced(external(3), CashAmount::All, "Bank", Some(zero), nop)),
        Refusal::BadText(_)
    ));
    let lower = RateNote {
        currency: *b"usd",
        minor_per_btc: 5,
    };
    assert!(matches!(
        refused(o.queue_cash_out_priced(external(3), CashAmount::All, "Bank", Some(lower), nop)),
        Refusal::BadText(_)
    ));
    ok(o.queue_cash_out_priced(external(3), CashAmount::All, "Bank", Some(rate(5)), nop));
    let bytes = o.snapshot().unwrap();
    // The price is the last 11 bytes of the cash-out list's entry, then the
    // (empty) refund list's count of 4 bytes follows.
    let mut cut = bytes.clone();
    cut.truncate(bytes.len() - 1);
    assert!(restore_owner(&cut).is_err());
    let mut zeroed = bytes.clone();
    let at = bytes.len() - 4 - 8;
    zeroed[at..at + 8].copy_from_slice(&[0; 8]);
    assert!(restore_owner(&zeroed).is_err());
    let mut padded = bytes.clone();
    padded.push(0);
    assert!(restore_owner(&padded).is_err());
}

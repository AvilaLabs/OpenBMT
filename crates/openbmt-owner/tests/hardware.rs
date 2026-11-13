//! A hardware signer as key 2 (AS-90 to AS-99), with the real crates on both
//! sides: the signer is played by the same signer state the Key app uses,
//! which reads and signs the file it is given as a hardware wallet does.

mod common;

use std::convert::Infallible;

use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::sighash::EcdsaSighashType;
use bitcoin::Transaction;
use common::*;
use openbmt_core::wallet::Keychain;
use openbmt_owner::hardware::{
    first_address, read_returned, registration, wallet_name, FirstAddressRecord, FirstAddressState,
    HardwareRefusal, Returned, SignerFile, MAGIC_FIRST_ADDRESS,
};
use openbmt_owner::{Delivery, Options, RefundPacking, Refusal, Stage};
use openbmt_session::{Item, ItemKind, Request};

const BIG: u64 = 1_000_000_000;

struct Fixture {
    rig: Rig,
    key_app: Vec<u8>,
    session: [u8; 32],
}

/// A refund of 40,000 and a cash-out in one build for a hardware wallet.
fn fixture() -> Fixture {
    let mut rig = Rig::with(Default::default(), false);
    refund(&mut rig.owner, 1, BIG, 40_000);
    let coins = [coin(1, 0, 500_000), coin(2, 1, 500_000)];
    let random = rig.random();
    let store = &rig.owner_store;
    let (built, signer) = ok(rig.owner.build_request_for_signer(
        &coins,
        1_000,
        0,
        rig.lock_height,
        NOW,
        &random,
        |b| store.put(b),
    ));
    Fixture {
        key_app: built.request,
        session: signer.expect("the refund is a spend for the signer"),
        rig,
    }
}

fn export(f: &mut Fixture) -> SignerFile {
    let store1 = &f.rig.key1_store;
    let store = &f.rig.owner_store;
    ok(f.rig.owner.export_for_signer(
        &f.session,
        0,
        &mut f.rig.key1,
        &key1_account(),
        NOW,
        |o, k| {
            store.put(o)?;
            store1.put(k)
        },
    ))
}

/// What a hardware signer returns for the file: its own signature on every
/// input, in a PSBT built from the unsigned transaction.
fn signer_signs(file: &[u8]) -> Vec<u8> {
    let mut signer = key2_signer();
    let review = signer.review(file, NOW).expect("the signer reads the file");
    let signed = signer
        .sign(
            review,
            &kit().accounts[1],
            NOW,
            |_| Ok::<(), Infallible>(()),
        )
        .map_err(|_| "signs")
        .expect("the signer signs");
    signed.psbt
}

fn take(f: &mut Fixture, returned: &[u8]) -> Result<openbmt_owner::hardware::SignerTaken, Refusal> {
    let store = &f.rig.owner_store;
    match f
        .rig
        .owner
        .take_from_signer(&f.session, 0, returned, |b| store.put(b))
    {
        Ok(t) => Ok(t),
        Err(openbmt_owner::OwnerError::Refused(r)) => Err(r),
        Err(e) => panic!("{e}"),
    }
}

fn hardware_refusal(r: Refusal) -> HardwareRefusal {
    match r {
        Refusal::Hardware(h) => h,
        other => panic!("not a hardware refusal: {other}"),
    }
}

#[test]
fn as_90_the_spends_go_to_a_session_of_their_own_and_not_to_the_key_app() {
    let f = fixture();
    // The Key app's request holds the health check and no spend.
    let kinds: Vec<ItemKind> = Request::from_bytes(&f.key_app)
        .unwrap()
        .items()
        .iter()
        .map(|i| i.item.kind())
        .collect();
    assert_eq!(kinds, [ItemKind::HealthCheck]);
    // The spend session holds one spend and no other item.
    let own = Request::from_bytes(f.rig.owner.session_request(&f.session).unwrap()).unwrap();
    assert_eq!(own.items().len(), 1);
    assert!(matches!(own.items()[0].item, Item::Spend { .. }));
    assert_eq!(f.rig.owner.open_sessions().len(), 2);
    // The refund waits in the session of the spend.
    assert!(matches!(
        f.rig.owner.refunds()[0].stage,
        Stage::InSession { session } if session == f.session
    ));
    // The plain build is unchanged: the spend is in the Key app's request.
    let mut plain = Rig::with(Default::default(), true);
    refund(&mut plain.owner, 1, BIG, 40_000);
    let built = plain.build(&[coin(1, 0, 500_000)], 0);
    let kinds: Vec<ItemKind> = Request::from_bytes(&built.request)
        .unwrap()
        .items()
        .iter()
        .map(|i| i.item.kind())
        .collect();
    assert_eq!(kinds, [ItemKind::Spend, ItemKind::HealthCheck]);
}

#[test]
fn as_90_nothing_for_the_signer_makes_no_second_session() {
    let mut rig = Rig::with(Default::default(), false);
    let random = rig.random();
    let store = &rig.owner_store;
    let (_, signer) = ok(rig.owner.build_request_for_signer(
        &[coin(1, 0, 500_000)],
        1_000,
        0,
        rig.lock_height,
        NOW,
        &random,
        |b| store.put(b),
    ));
    assert!(signer.is_none());
    assert_eq!(rig.owner.open_sessions().len(), 1);
}

#[test]
fn as_91_the_file_has_every_field_a_signer_reads() {
    let mut f = fixture();
    let file = export(&mut f);
    let psbt = Psbt::deserialize(&file.psbt).unwrap();
    assert_eq!(psbt.version, 0);
    assert_eq!(psbt.unsigned_tx.compute_txid(), file.txid);
    // The three keys, with their origins, for all of the wallet.
    assert_eq!(psbt.xpub.len(), 3);
    for slot in wallet().slots() {
        assert_eq!(
            psbt.xpub.get(&slot.xpub),
            Some(&(slot.origin_fingerprint, slot.origin_path.clone()))
        );
    }
    for (i, input) in psbt.inputs.iter().enumerate() {
        assert!(input.witness_utxo.is_some(), "input {i}");
        assert!(input.non_witness_utxo.is_some(), "input {i}");
        assert!(input.witness_script.is_some(), "input {i}");
        assert_eq!(input.bip32_derivation.len(), 3, "input {i}");
        assert_eq!(
            input.sighash_type.map(|t| t.ecdsa_hash_ty().unwrap()),
            Some(EcdsaSighashType::All)
        );
        // Key 1's signature is on it.
        assert_eq!(input.partial_sigs.len(), 1);
    }
    // The change output carries its script and three keys.
    let change = psbt
        .outputs
        .iter()
        .filter(|o| o.witness_script.is_some())
        .count();
    assert_eq!(change, usize::from(file.summary.change.is_some()));
    for o in psbt.outputs.iter().filter(|o| o.witness_script.is_some()) {
        assert_eq!(o.bip32_derivation.len(), 3);
    }
    assert_eq!(
        file.file_name,
        format!("{}.psbt", &file.txid.to_string()[..8])
    );
}

#[test]
fn as_91_the_file_is_kept_and_the_same_file_comes_again() {
    let mut f = fixture();
    let first = export(&mut f);
    let calls = f.rig.owner_store.calls();
    let k1_calls = f.rig.key1_store.calls();
    let again = export(&mut f);
    assert_eq!(again, first);
    // The second call stores nothing and signs nothing.
    assert_eq!(f.rig.owner_store.calls(), calls);
    assert_eq!(f.rig.key1_store.calls(), k1_calls);
    // The reading call gives the same file without a store.
    assert_eq!(f.rig.owner.signer_file(&f.session, 0).unwrap(), first);
    // It survives a restart of the phone.
    let restored = restore_owner(&f.rig.owner_store.last()).unwrap();
    assert_eq!(restored.signer_file(&f.session, 0).unwrap(), first);
}

#[test]
fn as_91_a_failed_store_changes_nothing() {
    let mut f = fixture();
    f.rig.owner_store.fail_next(true);
    let store = &f.rig.owner_store;
    let k1_before = f.rig.key1.to_bytes();
    let r = f.rig.owner.export_for_signer(
        &f.session,
        0,
        &mut f.rig.key1,
        &key1_account(),
        NOW,
        |o, _| store.put(o),
    );
    assert!(matches!(r, Err(openbmt_owner::OwnerError::Store(_))));
    assert_eq!(f.rig.key1.to_bytes(), k1_before);
    assert_eq!(
        f.rig.owner.signer_file(&f.session, 0).unwrap_err(),
        HardwareRefusal::NotExported
    );
    f.rig.owner_store.fail_next(false);
    export(&mut f);
}

#[test]
fn as_91_other_sessions_and_items_are_not_spends_for_the_signer() {
    let mut f = fixture();
    let key_app_session = f
        .rig
        .owner
        .open_sessions()
        .into_iter()
        .find(|s| *s != f.session)
        .unwrap();
    assert_eq!(
        f.rig.owner.signer_file(&key_app_session, 0).unwrap_err(),
        HardwareRefusal::NotASignerSpend
    );
    assert_eq!(
        f.rig.owner.signer_file(&f.session, 9).unwrap_err(),
        HardwareRefusal::NotASignerSpend
    );
    let store = &f.rig.owner_store;
    let r = f.rig.owner.export_for_signer(
        &[9; 32],
        0,
        &mut f.rig.key1,
        &key1_account(),
        NOW,
        |o, _| store.put(o),
    );
    assert!(matches!(
        r,
        Err(openbmt_owner::OwnerError::Refused(Refusal::Hardware(
            HardwareRefusal::NotASignerSpend
        )))
    ));
}

#[test]
fn as_92_the_summary_is_what_the_signer_shows() {
    let mut f = fixture();
    assert_eq!(
        f.rig.owner.signer_file(&f.session, 0).unwrap_err(),
        HardwareRefusal::NotExported
    );
    let file = export(&mut f);
    let psbt = Psbt::deserialize(&file.psbt).unwrap();
    let mut shown = file.summary.payments.clone();
    shown.extend(file.summary.change.clone());
    assert_eq!(shown.len(), psbt.unsigned_tx.output.len());
    let paid_total: u64 = shown.iter().map(|l| l.sats).sum();
    let spent: u64 = psbt
        .inputs
        .iter()
        .map(|i| i.witness_utxo.as_ref().unwrap().value.to_sat())
        .sum();
    assert_eq!(spent - paid_total, file.summary.fee);
    assert!(file.summary.fee > 0);
    // The refund's address and amount are the ones that were queued.
    assert!(file.summary.payments.iter().any(|l| l.sats == 40_000
        && l.address
            == bitcoin::Address::from_script(&external(1), NET)
                .unwrap()
                .to_string()));
}

#[test]
fn as_93_every_form_a_signer_returns_is_read() {
    let mut f = fixture();
    let file = export(&mut f);
    let signed = signer_signs(&file.psbt);
    // Binary.
    assert!(matches!(read_returned(&signed), Ok(Returned::Psbt(_))));
    // Base64 and hexadecimal text, with trailing whitespace.
    let b64 = base64(&signed);
    assert!(matches!(
        read_returned(format!("{b64}\n").as_bytes()),
        Ok(Returned::Psbt(_))
    ));
    assert!(matches!(
        read_returned(hex(&signed).as_bytes()),
        Ok(Returned::Psbt(_))
    ));
    // A finished transaction, in binary and in hexadecimal, and a PSBT whose
    // inputs are all final.
    let tx = finished_tx(&f, &file, &signed);
    let raw = bitcoin::consensus::serialize(&tx);
    assert_eq!(read_returned(&raw), Ok(Returned::Transaction(tx.clone())));
    assert_eq!(
        read_returned(hex(&raw).as_bytes()),
        Ok(Returned::Transaction(tx.clone()))
    );
    let mut fin = Psbt::deserialize(&file.psbt).unwrap();
    for (i, input) in fin.inputs.iter_mut().enumerate() {
        input.final_script_witness = Some(tx.input[i].witness.clone());
    }
    assert_eq!(
        read_returned(&fin.serialize()),
        Ok(Returned::Transaction(tx))
    );
    // Garbage, the empty file and an oversize one.
    for bad in [&b""[..], b"hello", &[0xff; 20], b"cHNidP8!"] {
        assert!(matches!(
            read_returned(bad),
            Err(HardwareRefusal::Unreadable(_))
        ));
    }
    assert!(read_returned(&vec![0u8; (4 << 20) + 1]).is_err());
}

#[test]
fn as_93_the_codes_carry_the_same_bytes() {
    let mut f = fixture();
    let file = export(&mut f);
    let signed = signer_signs(&file.psbt);
    // Animated UR, both type names.
    {
        let mut e = openbmt_ur::psbt_to_ur_parts(&signed, 120).unwrap();
        let mut d = openbmt_ur::Decoder::new();
        for _ in 0..(e.seq_len() * 4) {
            if d.receive(&e.next_part_upper()).unwrap().complete {
                break;
            }
        }
        let (kind, cbor) = d.result().unwrap();
        let bytes = openbmt_ur::ur_to_psbt(&kind, &cbor).unwrap();
        assert!(matches!(read_returned(&bytes), Ok(Returned::Psbt(_))));
    }
    // BBQr of type P, joined in the reverse order.
    let parts = openbmt_bbqr::encode(openbmt_bbqr::Kind::Psbt, &signed, 400).unwrap();
    let mut j =
        openbmt_bbqr::Joiner::new(&[openbmt_bbqr::Kind::Psbt, openbmt_bbqr::Kind::Transaction]);
    for p in parts.iter().rev() {
        j.add(p).unwrap();
    }
    let out = j.finish().unwrap();
    assert_eq!(out.bytes, signed);
    assert!(matches!(read_returned(&out.bytes), Ok(Returned::Psbt(_))));
}

#[test]
fn as_94_another_transaction_is_refused_in_each_form() {
    let mut f = fixture();
    let file = export(&mut f);
    // A second build gives another transaction for another session.
    refund(&mut f.rig.owner, 2, BIG, 12_345);
    let coins = [coin(7, 0, 700_000)];
    let random = f.rig.random();
    let store = &f.rig.owner_store;
    let (_, other_session) = ok(f.rig.owner.build_request_for_signer(
        &coins,
        1_000,
        5,
        f.rig.lock_height,
        NOW,
        &random,
        |b| store.put(b),
    ));
    let other = other_session.unwrap();
    let k1_store = &f.rig.key1_store;
    let other_file = ok(f.rig.owner.export_for_signer(
        &other,
        0,
        &mut f.rig.key1,
        &key1_account(),
        NOW,
        |o, k| {
            store.put(o)?;
            k1_store.put(k)
        },
    ));
    assert_ne!(other_file.txid, file.txid);
    let before = f.rig.owner.snapshot().unwrap();
    // A PSBT of the other transaction, signed by key 2.
    let wrong = signer_signs(&other_file.psbt);
    assert_eq!(
        hardware_refusal(take(&mut f, &wrong).unwrap_err()),
        HardwareRefusal::DifferentTransaction
    );
    // The other transaction, finished.
    let other_tx = {
        let psbt = Psbt::deserialize(&other_file.psbt).unwrap();
        let back = Psbt::deserialize(&wrong).unwrap();
        let both = openbmt_spend::combine(&psbt, &[back]).unwrap();
        openbmt_spend::finalize(wallet(), &both).unwrap()
    };
    assert_eq!(
        hardware_refusal(take(&mut f, &bitcoin::consensus::serialize(&other_tx)).unwrap_err()),
        HardwareRefusal::DifferentTransaction
    );
    assert_eq!(f.rig.owner.snapshot().unwrap(), before);
}

#[test]
fn as_95_a_file_without_the_signers_signature_is_refused() {
    let mut f = fixture();
    let file = export(&mut f);
    let before = f.rig.owner.snapshot().unwrap();
    // The file as it was given: only key 1 signed it.
    assert!(matches!(
        hardware_refusal(take(&mut f, &file.psbt).unwrap_err()),
        HardwareRefusal::NotSigned(_)
    ));
    // The unsigned transaction alone.
    let bare = Psbt::from_unsigned_tx(Psbt::deserialize(&file.psbt).unwrap().unsigned_tx)
        .unwrap()
        .serialize();
    assert!(matches!(
        hardware_refusal(take(&mut f, &bare).unwrap_err()),
        HardwareRefusal::NotSigned(_) | HardwareRefusal::Finished(_)
    ));
    // A signature of key 3 in place of key 2's.
    let mut signer3 = openbmt_signer::SignerState::register(
        wallet().canonical_string(),
        NET,
        &kit().xpubs[2],
        Some(&kit().xpubs[0]),
    )
    .unwrap();
    let review = signer3.review(&file.psbt, NOW).unwrap();
    let by3 = signer3
        .sign(
            review,
            &kit().accounts[2],
            NOW,
            |_| Ok::<(), Infallible>(()),
        )
        .map_err(|_| "signs")
        .unwrap()
        .psbt;
    assert!(matches!(
        hardware_refusal(take(&mut f, &by3).unwrap_err()),
        HardwareRefusal::NotSigned(_)
    ));
    assert_eq!(f.rig.owner.snapshot().unwrap(), before);
}

fn finished_tx(f: &Fixture, file: &SignerFile, signed: &[u8]) -> Transaction {
    let _ = f;
    let sent = Psbt::deserialize(&file.psbt).unwrap();
    let back = Psbt::deserialize(signed).unwrap();
    let both = openbmt_spend::combine(&sent, &[back]).unwrap();
    openbmt_spend::finalize(wallet(), &both).unwrap()
}

#[test]
fn as_96_a_signed_psbt_is_finished_and_waits_to_be_sent() {
    let mut f = fixture();
    let file = export(&mut f);
    let signed = signer_signs(&file.psbt);
    let taken = take(&mut f, &signed).unwrap();
    assert_eq!(taken.txid, file.txid);
    assert!(taken.fee > 0);
    let out = f.rig.owner.outbox();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].id, taken.id);
    let Delivery::Transaction {
        txid, tx, refunds, ..
    } = &out[0].what
    else {
        panic!("a transaction")
    };
    assert_eq!(*txid, file.txid);
    let tx: Transaction = bitcoin::consensus::deserialize(tx).unwrap();
    assert_eq!(tx.compute_txid(), file.txid);
    assert_eq!(tx.input[0].witness.len(), 4);
    assert_eq!(refunds.len(), 1);
    assert!(matches!(
        f.rig.owner.refunds()[0].stage,
        Stage::Signed { txid } if txid == file.txid
    ));
    // The session ended: only the Key app's remains.
    assert!(!f.rig.owner.open_sessions().contains(&f.session));
    // The follow-up is the same as for a transaction the Key app approved.
    ok(f.rig
        .owner
        .mark_broadcast(&file.txid, |b| f.rig.owner_store.put(b)));
}

#[test]
fn as_96_a_finished_transaction_is_taken_when_it_is_the_one_given() {
    let mut f = fixture();
    let file = export(&mut f);
    let signed = signer_signs(&file.psbt);
    let tx = finished_tx(&f, &file, &signed);
    let taken = take(&mut f, &bitcoin::consensus::serialize(&tx)).unwrap();
    assert_eq!(taken.txid, file.txid);
    assert_eq!(f.rig.owner.outbox().len(), 1);
}

#[test]
fn as_96_a_tampered_witness_is_refused_and_nothing_changes() {
    let mut f = fixture();
    let file = export(&mut f);
    let signed = signer_signs(&file.psbt);
    let mut tx = finished_tx(&f, &file, &signed);
    let mut w: Vec<Vec<u8>> = tx.input[0].witness.iter().map(<[u8]>::to_vec).collect();
    w[1][10] ^= 1;
    tx.input[0].witness = bitcoin::Witness::from_slice(&w);
    let before = f.rig.owner.snapshot().unwrap();
    let r = take(&mut f, &bitcoin::consensus::serialize(&tx)).unwrap_err();
    assert!(matches!(
        hardware_refusal(r),
        HardwareRefusal::Finished(_) | HardwareRefusal::DifferentTransaction
    ));
    assert_eq!(f.rig.owner.snapshot().unwrap(), before);
    // The right file can be brought after the refusal.
    take(&mut f, &signed).unwrap();
}

#[test]
fn as_96_a_failed_store_changes_nothing_and_a_full_outbox_refuses() {
    let mut f = fixture();
    let file = export(&mut f);
    let signed = signer_signs(&file.psbt);
    f.rig.owner_store.fail_next(true);
    let store = &f.rig.owner_store;
    let r = f
        .rig
        .owner
        .take_from_signer(&f.session, 0, &signed, |b| store.put(b));
    assert!(matches!(r, Err(openbmt_owner::OwnerError::Store(_))));
    assert!(f.rig.owner.outbox().is_empty());
    f.rig.owner_store.fail_next(false);
    take(&mut f, &signed).unwrap();
    // The session is gone: the same file cannot be taken twice.
    assert!(matches!(
        hardware_refusal(take(&mut f, &signed).unwrap_err()),
        HardwareRefusal::NotASignerSpend
    ));
}

#[test]
fn as_96_without_an_export_nothing_is_taken() {
    let mut f = fixture();
    let before = f.rig.owner.snapshot().unwrap();
    assert_eq!(
        hardware_refusal(take(&mut f, b"psbt\xff").unwrap_err()),
        HardwareRefusal::NotExported
    );
    assert_eq!(f.rig.owner.snapshot().unwrap(), before);
}

/// The fixture wallet's registration, as the text a signer is given.
const REGISTRATION_TEXT: &str =
    "Name: Corner Cafe\nPolicy: 2 of 3\nDerivation: m/48'/1'/0'/2'\nFormat: P2WSH\n\n";

#[test]
fn as_97_the_text_file_has_the_lines_a_signer_reads() {
    let r = registration(wallet(), "Corner Cafe!").unwrap();
    assert_eq!(r.name, "Corner Cafe");
    assert!(r.text_file.starts_with(REGISTRATION_TEXT));
    let keys: Vec<&str> = r.text_file[REGISTRATION_TEXT.len()..].lines().collect();
    assert_eq!(keys.len(), 3);
    for (line, slot) in keys.iter().zip(wallet().slots()) {
        let (fp, xpub) = line.split_once(": ").unwrap();
        assert_eq!(fp, slot.origin_fingerprint.to_string().to_uppercase());
        assert_eq!(fp.len(), 8);
        assert!(fp
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));
        assert_eq!(xpub, slot.xpub.to_string());
        assert!(xpub.starts_with("tpub"));
    }
    assert!(r
        .text_file
        .lines()
        .all(|l| l.chars().all(|c| (' '..='~').contains(&c))));
}

#[test]
fn as_97_the_name_is_printable_and_short() {
    assert_eq!(
        wallet_name("Corner Cafe & Bar \u{1f600}"),
        "Corner Cafe  Ba"
    );
    assert_eq!(wallet_name("   "), "OpenBMT");
    assert_eq!(wallet_name("\u{e9}\u{e9}"), "OpenBMT");
    let long = wallet_name("abcdefghijklmnopqrstuvwxyz");
    assert_eq!(long.len(), 15);
    assert!(wallet_name("a\nb: c")
        .chars()
        .all(|c| c != '\n' && c != ':'));
}

#[test]
fn as_97_the_descriptors_have_their_checksums_and_forms() {
    let r = registration(wallet(), "Shop").unwrap();
    assert_eq!(r.descriptor_both, wallet().canonical_string());
    assert!(r.descriptor_both.contains("<0;1>/*"));
    assert!(r.descriptor_receive.contains("/0/*") && !r.descriptor_receive.contains('<'));
    assert!(r.descriptor_change.contains("/1/*"));
    for d in [
        &r.descriptor_both,
        &r.descriptor_receive,
        &r.descriptor_change,
    ] {
        let (body, sum) = d.rsplit_once('#').unwrap();
        assert_eq!(
            openbmt_core::wallet::descriptor_checksum(body).unwrap(),
            sum
        );
        assert!(body.starts_with("wsh(sortedmulti(2,"));
    }
}

#[test]
fn as_97_the_ur_output_reads_back_as_the_three_keys() {
    let r = registration(wallet(), "Shop").unwrap();
    let mut e = openbmt_ur::output_to_ur_parts(&r.output_cbor, 150).unwrap();
    let mut d = openbmt_ur::Decoder::new();
    for _ in 0..(e.seq_len() * 4) {
        if d.receive(&e.next_part_upper()).unwrap().complete {
            break;
        }
    }
    let (kind, cbor) = d.result().unwrap();
    assert_eq!(kind, openbmt_ur::TYPE_CRYPTO_OUTPUT);
    assert_eq!(cbor, r.output_cbor);
    // Each key is the wallet's, with its origin.
    let k = cbor.windows(2).filter(|w| w == &[0xd9, 0x01]).count();
    assert!(k >= 4);
}

#[test]
fn as_98_a_short_text_is_one_plain_code_and_a_long_one_is_not() {
    let r = registration(wallet(), "Shop").unwrap();
    assert!(r.text_fits_plain_qr());
    assert!(r.descriptor_fits_plain_qr());
    assert!(r.text_file.len() < 1500);
    // Over the other transports the text comes back byte for byte.
    let parts =
        openbmt_bbqr::encode(openbmt_bbqr::Kind::Text, r.text_file.as_bytes(), 200).unwrap();
    let mut j = openbmt_bbqr::Joiner::new(&[openbmt_bbqr::Kind::Text]);
    for p in &parts {
        j.add(p).unwrap();
    }
    assert_eq!(j.finish().unwrap().bytes, r.text_file.as_bytes());
    let mut e = openbmt_ur::text_to_ur_parts(&r.text_file, 100).unwrap();
    let mut d = openbmt_ur::Decoder::new();
    for _ in 0..(e.seq_len() * 4) {
        if d.receive(&e.next_part_upper()).unwrap().complete {
            break;
        }
    }
    assert_eq!(d.result().unwrap().0, openbmt_ur::TYPE_BYTES);
}

#[test]
fn as_99_the_first_address_is_receive_address_zero() {
    let a = first_address(wallet()).unwrap();
    assert_eq!(
        a,
        wallet().address(Keychain::Receive, 0).unwrap().to_string()
    );
    assert!(a.starts_with("bcrt1q"));
}

#[test]
fn as_99_the_record_keeps_done_or_skipped_with_the_time() {
    for state in [FirstAddressState::Checked, FirstAddressState::Skipped] {
        let rec = FirstAddressRecord::new(wallet(), state, 1_700_000_123);
        let bytes = rec.to_bytes();
        assert_eq!(&bytes[..4], &MAGIC_FIRST_ADDRESS);
        assert_eq!(bytes.len(), 45);
        assert_eq!(FirstAddressRecord::from_bytes(wallet(), &bytes), Ok(rec));
    }
}

#[test]
fn as_99_a_record_that_is_not_this_wallets_or_not_framed_is_refused() {
    let good = FirstAddressRecord::new(wallet(), FirstAddressState::Checked, 5).to_bytes();
    let mut other = good.clone();
    other[10] ^= 1;
    assert!(matches!(
        FirstAddressRecord::from_bytes(wallet(), &other),
        Err(HardwareRefusal::Record(_))
    ));
    let mut magic = good.clone();
    magic[3] = b'1';
    let mut state = good.clone();
    state[36] = 3;
    for bad in [
        magic,
        state,
        good[..44].to_vec(),
        [good.clone(), vec![0]].concat(),
        Vec::new(),
    ] {
        assert!(FirstAddressRecord::from_bytes(wallet(), &bad).is_err());
    }
}

// ---- AS-88: taking one payment keeps the rest of the session -------------------------

/// Two refunds, each in a transaction of its own, in one session for the
/// signer.
fn two_spends() -> Fixture {
    let options = Options {
        packing: RefundPacking::OneEach,
        ..Default::default()
    };
    let mut rig = Rig::with(options, false);
    refund(&mut rig.owner, 1, BIG, 40_000);
    refund(&mut rig.owner, 2, BIG, 30_000);
    let coins = [coin(1, 0, 500_000), coin(2, 1, 500_000)];
    let random = rig.random();
    let store = &rig.owner_store;
    let (built, signer) = ok(rig.owner.build_request_for_signer(
        &coins,
        1_000,
        0,
        rig.lock_height,
        NOW,
        &random,
        |b| store.put(b),
    ));
    Fixture {
        key_app: built.request,
        session: signer.expect("the refunds are spends for the signer"),
        rig,
    }
}

fn numbers(f: &Fixture) -> Vec<u32> {
    Request::from_bytes(f.rig.owner.session_request(&f.session).unwrap())
        .unwrap()
        .items()
        .iter()
        .map(|i| i.number)
        .collect()
}

fn export_item(f: &mut Fixture, number: u32) -> SignerFile {
    let store1 = &f.rig.key1_store;
    let store = &f.rig.owner_store;
    ok(f.rig.owner.export_for_signer(
        &f.session,
        number,
        &mut f.rig.key1,
        &key1_account(),
        NOW,
        |o, k| {
            store.put(o)?;
            store1.put(k)
        },
    ))
}

fn take_item(f: &mut Fixture, number: u32) -> openbmt_owner::hardware::SignerTaken {
    let file = export_item(f, number);
    let signed = signer_signs(&file.psbt);
    let store = &f.rig.owner_store;
    ok(f.rig
        .owner
        .take_from_signer(&f.session, number, &signed, |b| store.put(b)))
}

/// The owner phone after a restart: what was stored, read back.
fn restart(f: &mut Fixture) {
    f.rig.owner = restore_owner(&f.rig.owner_store.last()).expect("the stored queue restores");
}

#[test]
fn as_88_taking_one_payment_keeps_the_other_in_the_session() {
    let mut f = two_spends();
    let n = numbers(&f);
    assert_eq!(n.len(), 2);
    let first = take_item(&mut f, n[0]);
    // The session stays, with the second payment only.
    assert!(f.rig.owner.open_sessions().contains(&f.session));
    assert_eq!(numbers(&f), [n[1]]);
    assert!(matches!(
        f.rig.owner.refunds()[0].stage,
        Stage::Signed { txid } if txid == first.txid
    ));
    assert!(matches!(
        f.rig.owner.refunds()[1].stage,
        Stage::InSession { session } if session == f.session
    ));
    // A restart reads the queue back, and the second can be taken.
    restart(&mut f);
    assert_eq!(numbers(&f), [n[1]]);
    let second = take_item(&mut f, n[1]);
    assert_ne!(second.txid, first.txid);
    assert_eq!(f.rig.owner.outbox().len(), 2);
    // The last item ends the session.
    assert!(!f.rig.owner.open_sessions().contains(&f.session));
    restart(&mut f);
    assert!(!f.rig.owner.open_sessions().contains(&f.session));
}

#[test]
fn as_88_the_second_payment_can_be_abandoned_after_the_first_is_taken() {
    let mut f = two_spends();
    let n = numbers(&f);
    take_item(&mut f, n[0]);
    restart(&mut f);
    let store = &f.rig.owner_store;
    ok(f.rig.owner.abandon_session(&f.session, |b| store.put(b)));
    assert!(!f.rig.owner.open_sessions().contains(&f.session));
    assert_eq!(f.rig.owner.refunds()[1].stage, Stage::Queued);
    // The first payment is untouched, and the queue reads back.
    assert!(matches!(
        f.rig.owner.refunds()[0].stage,
        Stage::Signed { .. }
    ));
    restart(&mut f);
    assert_eq!(f.rig.owner.outbox().len(), 1);
    assert!(f.rig.owner.signed_elsewhere().is_empty());
}

// ---- AS-89: abandoning an exported approval can never pay twice -----------------------

fn inputs_of(file: &SignerFile) -> Vec<bitcoin::OutPoint> {
    let mut v: Vec<_> = Psbt::deserialize(&file.psbt)
        .unwrap()
        .unsigned_tx
        .input
        .iter()
        .map(|i| i.previous_output)
        .collect();
    v.sort();
    v
}

fn spent_by(f: &Fixture, session: &[u8; 32]) -> Vec<bitcoin::OutPoint> {
    let request = Request::from_bytes(f.rig.owner.session_request(session).unwrap()).unwrap();
    let mut v = Vec::new();
    for item in request.items() {
        if let Item::Spend { psbt, .. } = &item.item {
            v.extend(
                Psbt::deserialize(psbt)
                    .unwrap()
                    .unsigned_tx
                    .input
                    .iter()
                    .map(|i| i.previous_output),
            );
        }
    }
    v.sort();
    v
}

fn abandon(f: &mut Fixture) {
    let store = &f.rig.owner_store;
    ok(f.rig.owner.abandon_session(&f.session, |b| store.put(b)));
}

/// The refund is built again from `coins`, in a session of its own.
fn rebuild(f: &mut Fixture, coins: &[openbmt_spend::Coin]) -> [u8; 32] {
    let random = f.rig.random();
    let store = &f.rig.owner_store;
    let (_, signer) = ok(f.rig.owner.build_request_for_signer(
        coins,
        1_000,
        1,
        f.rig.lock_height,
        NOW,
        &random,
        |b| store.put(b),
    ));
    signer.expect("the refund is built again")
}

/// A coin with `confirmations`.
fn aged(tag: u32, index: u32, confirmations: u32) -> openbmt_spend::Coin {
    let mut c = coin(tag, index, 500_000);
    c.confirmations = confirmations;
    c
}

#[test]
fn as_89_an_exported_approval_that_is_abandoned_is_kept_as_signed_elsewhere() {
    let mut f = fixture();
    let file = export(&mut f);
    abandon(&mut f);
    let kept = f.rig.owner.signed_elsewhere().to_vec();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].txid, file.txid);
    assert_eq!(kept[0].refunds, [f.rig.owner.refunds()[0].id]);
    assert_eq!(kept[0].cash_out, None);
    assert_eq!(kept[0].inputs, inputs_of(&file));
    // The payment is back in the queue, carrying what the old one spends.
    assert_eq!(f.rig.owner.refunds()[0].stage, Stage::Queued);
    assert_eq!(f.rig.owner.refunds()[0].conflict, inputs_of(&file));
    // It cannot be cancelled while the old transaction may confirm.
    let id = f.rig.owner.refunds()[0].id;
    let store = &f.rig.owner_store;
    assert!(f.rig.owner.cancel(id, |b| store.put(b)).is_err());
    // A restart keeps all of it.
    restart(&mut f);
    assert_eq!(f.rig.owner.signed_elsewhere(), kept.as_slice());
    assert_eq!(f.rig.owner.refunds()[0].conflict, inputs_of(&file));
}

#[test]
fn as_89_an_approval_abandoned_before_the_export_behaves_as_before() {
    let mut f = fixture();
    abandon(&mut f);
    assert!(f.rig.owner.signed_elsewhere().is_empty());
    assert_eq!(f.rig.owner.refunds()[0].stage, Stage::Queued);
    assert!(f.rig.owner.refunds()[0].conflict.is_empty());
    // The queue is written as before: version 1.
    assert_eq!(f.rig.owner_store.last()[4], 1);
}

#[test]
fn as_89_the_old_transaction_confirming_pays_the_refund_once() {
    let mut f = fixture();
    let file = export(&mut f);
    abandon(&mut f);
    assert_eq!(f.rig.owner_store.last()[4], 4);
    restart(&mut f);
    let id = f.rig.owner.refunds()[0].id;
    let store = &f.rig.owner_store;
    let dropped = ok(f
        .rig
        .owner
        .mark_elsewhere_confirmed(&file.txid, |b| store.put(b)));
    assert!(dropped.is_empty());
    assert!(f.rig.owner.signed_elsewhere().is_empty());
    assert_eq!(f.rig.owner.refunds()[0].id, id);
    assert_eq!(
        f.rig.owner.refunds()[0].stage,
        Stage::Paid { txid: file.txid }
    );
    // Nothing is left to build: the refund is not paid a second time, from
    // the same coins or from others.
    let coins = [
        coin(1, 0, 500_000),
        coin(2, 1, 500_000),
        coin(3, 2, 500_000),
    ];
    let random = f.rig.random();
    let store = &f.rig.owner_store;
    let (_, signer) = ok(f.rig.owner.build_request_for_signer(
        &coins,
        1_000,
        1,
        f.rig.lock_height,
        NOW,
        &random,
        |b| store.put(b),
    ));
    assert!(signer.is_none());
    // The same report twice is refused.
    assert!(f
        .rig
        .owner
        .mark_elsewhere_confirmed(&file.txid, |b| store.put(b))
        .is_err());
    // It is paid for good: it archives, and nothing pays it again.
    let n = ok(f
        .rig
        .owner
        .archive_paid(&Default::default(), |b| store.put(b)));
    assert_eq!(n, 1);
    assert!(f.rig.owner.refunds().is_empty());
}

#[test]
fn as_89_a_new_transaction_before_the_old_confirms_conflicts_with_it() {
    let mut f = fixture();
    let file = export(&mut f);
    abandon(&mut f);
    let old = inputs_of(&file);
    // The coin the old transaction spends is the youngest now, and another
    // is much older, which the planner would prefer.
    let first = old[0];
    let (young, older) = if first == coin(1, 0, 500_000).outpoint {
        (aged(1, 0, 1), aged(2, 1, 9))
    } else {
        (aged(2, 1, 1), aged(1, 0, 9))
    };
    assert_eq!(young.outpoint, first);
    let session = rebuild(&mut f, &[young, older]);
    let new = spent_by(&f, &session);
    assert!(
        new.iter().any(|o| old.contains(o)),
        "the new transaction spends one of the old one's coins"
    );
}

#[test]
fn as_89_when_the_old_coins_are_not_among_the_coins_the_entry_is_planned_freely() {
    let mut f = fixture();
    let file = export(&mut f);
    abandon(&mut f);
    // The old coins are not among the coins handed in: the protection holds
    // only while one is, and the entry is planned freely once none is
    // (AS-68).
    let old = inputs_of(&file);
    let others: Vec<_> = [coin(3, 2, 500_000), coin(4, 3, 500_000)]
        .into_iter()
        .filter(|c| !old.contains(&c.outpoint))
        .collect();
    let session = rebuild(&mut f, &others);
    assert!(!spent_by(&f, &session).is_empty());
}

#[test]
fn as_89_the_old_transaction_confirming_after_a_new_one_was_built_ends_the_new_one() {
    let mut f = fixture();
    let file = export(&mut f);
    abandon(&mut f);
    let old = inputs_of(&file);
    let coins: Vec<_> = [coin(1, 0, 500_000), coin(2, 1, 500_000)].to_vec();
    // The new one is in an open approval when the old one confirms.
    let session = rebuild(&mut f, &coins);
    assert!(spent_by(&f, &session).iter().any(|o| old.contains(o)));
    let store = &f.rig.owner_store;
    let dropped = ok(f
        .rig
        .owner
        .mark_elsewhere_confirmed(&file.txid, |b| store.put(b)));
    assert!(dropped.is_empty());
    assert_eq!(
        f.rig.owner.refunds()[0].stage,
        Stage::Paid { txid: file.txid }
    );
    assert!(!f.rig.owner.open_sessions().contains(&session));
    restart(&mut f);
    assert_eq!(
        f.rig.owner.refunds()[0].stage,
        Stage::Paid { txid: file.txid }
    );
}

#[test]
fn as_89_the_old_transaction_confirming_after_a_new_one_was_signed_drops_the_new_one() {
    let mut f = fixture();
    let file = export(&mut f);
    abandon(&mut f);
    let coins: Vec<_> = [coin(1, 0, 500_000), coin(2, 1, 500_000)].to_vec();
    f.session = rebuild(&mut f, &coins);
    let new = take_item(&mut f, 0);
    assert_ne!(new.txid, file.txid);
    let store = &f.rig.owner_store;
    ok(f.rig.owner.mark_broadcast(&new.txid, |b| store.put(b)));
    let dropped = ok(f
        .rig
        .owner
        .mark_elsewhere_confirmed(&file.txid, |b| store.put(b)));
    assert_eq!(dropped, [new.txid]);
    assert_eq!(
        f.rig.owner.refunds()[0].stage,
        Stage::Paid { txid: file.txid }
    );
    assert!(f.rig.owner.outbox().is_empty());
    restart(&mut f);
    assert!(f.rig.owner.outbox().is_empty());
}

#[test]
fn as_89_another_transaction_spending_its_coin_releases_the_protection() {
    let mut f = fixture();
    let file = export(&mut f);
    abandon(&mut f);
    let store = &f.rig.owner_store;
    ok(f.rig
        .owner
        .mark_elsewhere_replaced(&file.txid, |b| store.put(b)));
    assert!(f.rig.owner.signed_elsewhere().is_empty());
    assert_eq!(f.rig.owner.refunds()[0].stage, Stage::Queued);
    assert!(f
        .rig
        .owner
        .mark_elsewhere_replaced(&file.txid, |b| store.put(b))
        .is_err());
    // The coins it spent are gone for good, so the refund is planned freely
    // from others (AS-68), and the old one can not confirm.
    let old = inputs_of(&file);
    let others: Vec<_> = [coin(3, 2, 500_000), coin(4, 3, 500_000)].to_vec();
    assert!(others.iter().all(|c| !old.contains(&c.outpoint)));
    let session = rebuild(&mut f, &others);
    assert!(spent_by(&f, &session).iter().all(|o| !old.contains(o)));
    restart(&mut f);
    assert!(f.rig.owner.signed_elsewhere().is_empty());
}

#[test]
fn as_89_an_exported_transaction_confirming_with_its_approval_open_pays_and_ends_it() {
    let mut f = fixture();
    let file = export(&mut f);
    assert_eq!(f.rig.owner.exported_transactions(None).len(), 1);
    let store = &f.rig.owner_store;
    ok(f.rig
        .owner
        .mark_exported_confirmed(&file.txid, |b| store.put(b)));
    assert_eq!(
        f.rig.owner.refunds()[0].stage,
        Stage::Paid { txid: file.txid }
    );
    assert!(!f.rig.owner.open_sessions().contains(&f.session));
    assert!(f.rig.owner.exported_transactions(None).is_empty());
    assert!(f.rig.owner.signed_elsewhere().is_empty());
    restart(&mut f);
    assert_eq!(
        f.rig.owner.refunds()[0].stage,
        Stage::Paid { txid: file.txid }
    );
    // Not exported, or not known: refused.
    let store = &f.rig.owner_store;
    assert!(f
        .rig
        .owner
        .mark_exported_confirmed(&file.txid, |b| store.put(b))
        .is_err());
}

#[test]
fn as_89_a_failed_store_changes_nothing() {
    let mut f = fixture();
    export(&mut f);
    let before = f.rig.owner.snapshot().unwrap();
    f.rig.owner_store.fail_next(true);
    let store = &f.rig.owner_store;
    assert!(f
        .rig
        .owner
        .abandon_session(&f.session, |b| store.put(b))
        .is_err());
    assert_eq!(f.rig.owner.snapshot().unwrap(), before);
    assert!(f.rig.owner.signed_elsewhere().is_empty());
    assert!(f.rig.owner.open_sessions().contains(&f.session));
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in b.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, x)| a | u32::from(*x) << (16 - 8 * i));
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(A[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn oc_55_a_held_transaction_is_stored_as_version_6_and_an_older_queue_still_reads() {
    let mut f = fixture();
    let file = export(&mut f);
    abandon(&mut f);
    // Watched, nothing held: version 4, as before, and it reads.
    assert_eq!(f.rig.owner_store.last()[4], 4);
    restart(&mut f);
    assert!(f.rig.owner.held_entries().is_empty());
    let held = openbmt_owner::Held {
        by: bitcoin::Txid::from_byte_array([9; 32]),
        block: [3; 32],
        height: 7,
    };
    let store = &f.rig.owner_store;
    ok(f.rig
        .owner
        .hold_elsewhere(&file.txid, held, false, |b| store.put(b)));
    assert_eq!(f.rig.owner_store.last()[4], 6);
    restart(&mut f);
    assert_eq!(f.rig.owner.signed_elsewhere()[0].held, Some(held));
    let id = f.rig.owner.refunds()[0].id;
    assert_eq!(f.rig.owner.held_entries().len(), 1);
    assert_eq!(f.rig.owner.held_entries()[0].id, id);
    // Held again with another block changes nothing while its block stands.
    let other = openbmt_owner::Held {
        block: [4; 32],
        ..held
    };
    let store = &f.rig.owner_store;
    ok(f.rig
        .owner
        .hold_elsewhere(&file.txid, other, false, |b| store.put(b)));
    assert_eq!(f.rig.owner.signed_elsewhere()[0].held, Some(held));
    // And takes the new block once its own has left.
    let store = &f.rig.owner_store;
    ok(f.rig
        .owner
        .hold_elsewhere(&file.txid, other, true, |b| store.put(b)));
    assert_eq!(f.rig.owner.signed_elsewhere()[0].held, Some(other));
    // It cannot be cancelled.
    let store = &f.rig.owner_store;
    assert!(f.rig.owner.cancel(id, |b| store.put(b)).is_err());
    // Released: no longer watched, and the queue is version 1 again.
    let store = &f.rig.owner_store;
    ok(f.rig.owner.release_held(&file.txid, |b| store.put(b)));
    assert!(f.rig.owner.signed_elsewhere().is_empty());
    assert!(f.rig.owner.held_entries().is_empty());
    let store = &f.rig.owner_store;
    assert!(f
        .rig
        .owner
        .release_held(&file.txid, |b| store.put(b))
        .is_err());
}

#[test]
fn oc_55_a_held_entry_is_built_only_with_a_coin_that_conflicts_with_the_beaten_transaction() {
    let mut f = fixture();
    let file = export(&mut f);
    abandon(&mut f);
    // A coin that is an output of the transaction that beat it.
    let by_coin = coin(5, 0, 500_000);
    let held = openbmt_owner::Held {
        by: by_coin.outpoint.txid,
        block: [3; 32],
        height: 7,
    };
    let store = &f.rig.owner_store;
    ok(f.rig
        .owner
        .hold_elsewhere(&file.txid, held, false, |b| store.put(b)));
    let build = |f: &mut Fixture, coins: &[openbmt_spend::Coin]| {
        let random = f.rig.random();
        let store = &f.rig.owner_store;
        let (built, signer) = ok(f.rig.owner.build_request_for_signer(
            coins,
            1_000,
            1,
            f.rig.lock_height,
            NOW,
            &random,
            |b| store.put(b),
        ));
        (built.skipped, signer)
    };
    // A free coin of the wallet: the entry stays held and says so.
    let (skipped, signer) = build(&mut f, &[coin(3, 0, 500_000)]);
    assert!(signer.is_none());
    assert_eq!(skipped.len(), 1);
    assert!(skipped[0].reason.contains("WORDS_NEEDED_OC_55_HELD"));
    // A coin of the transaction that beat it is enough.
    let (skipped, signer) = build(&mut f, &[coin(3, 0, 500_000), by_coin.clone()]);
    assert!(skipped.is_empty(), "{skipped:?}");
    assert!(signer.is_some());
}

//! Section 7: refunds, the cashier's side (CR-60 to CR-74).

use bitcoin::hashes::Hash;
use bitcoin::{Address, Network, ScriptBuf};
use openbmt_core::records::RecordKind;

use super::kit::*;
use crate::{
    refund_destination, Actor, Amount, Ask, CashRefund, DestinationRefused, Event, RefundAsk,
    RefundRefused, RefundRequest, RefundState, Role, Slot, WithdrawRefused,
};

fn ann() -> Actor {
    Actor {
        role: Role::Cashier,
        name: "ann".to_string(),
    }
}

fn max() -> Actor {
    Actor {
        role: Role::Manager,
        name: "max".to_string(),
    }
}

/// A script that is not in any of the counter's batches (which hold
/// `p2wsh(1)` to `p2wsh(100)`).
fn dest() -> ScriptBuf {
    p2wsh(0xa1)
}

fn request(sats: u64, actor: Actor) -> RefundRequest {
    RefundRequest {
        ask: RefundAsk::Sats(sats),
        destination: dest(),
        actor,
        reason: "wrong size".to_string(),
    }
}

fn cash(minor: u64, actor: Actor) -> CashRefund {
    CashRefund {
        actor,
        minor: Amount(minor),
        reason: "till refund".to_string(),
    }
}

/// A sale of `cents` that was paid `sats` and confirmed in a block at
/// `height` (tip 100), and judged once.
fn paid_sale(w: &mut World, cents: u64, sats: u64, height: u32) -> u32 {
    let o = w.open(cents);
    let t = w.pay(&o, sats);
    w.confirm(&[t], height);
    w.judge(o.index);
    o.index
}

/// A $5.00 sale paid in full that has reached the refund depth.
fn final_sale(w: &mut World) -> u32 {
    paid_sale(w, 500, 5_000, 95)
}

fn ask(
    w: &mut World,
    index: u32,
    sats: u64,
    actor: Actor,
) -> Result<u64, RefundRefused<&'static str>> {
    w.c.request_refund(index, request(sats, actor), w.now, &w.chain, &mut w.m)
}

fn ask_cash(
    w: &mut World,
    index: u32,
    minor: u64,
    actor: Actor,
) -> Result<(), RefundRefused<&'static str>> {
    w.c.refund_in_cash(index, cash(minor, actor), w.now, &w.chain, &mut w.m)
}

fn withdraw(
    w: &mut World,
    index: u32,
    number: u64,
    actor: Actor,
) -> Result<(), WithdrawRefused<&'static str>> {
    w.c.withdraw_refund(index, number, actor, w.now, &mut w.m)
}

fn refund_events(w: &World) -> Vec<Event> {
    w.m.events()
        .into_iter()
        .filter(|e| e.kind() == RecordKind::Refund)
        .collect()
}

// CR-60.

#[test]
fn cr_60_a_request_is_a_record_of_kind_refund_and_moves_nothing() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    let before = w.c.exposure_outstanding();
    let watched = w.c.watched_scripts();
    w.m.reset();
    let number = ask(&mut w, i, 1_000, ann()).unwrap();
    assert_eq!(number, 1);
    // The desk with the number, the record, the sale: nothing else is
    // written, and the counter holds nothing that pays.
    assert_eq!(w.m.log, ["Desk", "record", "Sale(0)"]);
    assert_eq!(w.c.exposure_outstanding(), before);
    assert_eq!(w.c.watched_scripts(), watched);
    let last = w.c.records_len() - 1;
    let record = w.c.record_at(last, &w.m).unwrap().unwrap();
    assert_eq!(record.kind, RecordKind::Refund);
    assert!(matches!(
        Event::decode(&record.body).unwrap(),
        Event::RefundRequested { number: 1, .. }
    ));
    // It is on the counter's own signed chain, which the owner verifies.
    let devices = openbmt_core::records::DeviceList::new([id(3).device_id()]);
    let all = w.c.records_from(0, usize::MAX, &w.m).unwrap();
    openbmt_core::records::Chain::from_records(all)
        .verify(&devices)
        .unwrap();
}

// CR-61.

#[test]
fn cr_61_a_request_names_what_the_owner_needs() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    w.tick(30);
    ask(&mut w, i, 1_200, ann()).unwrap();
    let events = refund_events(&w);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0],
        Event::RefundRequested {
            index: i,
            at: w.now,
            number: 1,
            role: Role::Cashier,
            name: "ann".to_string(),
            reason: "wrong size".to_string(),
            destination: dest(),
            received_sats: 5_000,
            waiting_for_depth: false,
            sats: 1_200,
            ask: Ask::Sats,
            chain_height: Some(100),
        }
    );
}

#[test]
fn cr_61_request_numbers_rise_from_one_per_counter_and_survive_a_restart() {
    let mut w = World::new();
    let a = final_sale(&mut w);
    let b = final_sale(&mut w);
    assert_eq!(ask(&mut w, a, 1_000, ann()), Ok(1));
    assert_eq!(ask(&mut w, b, 1_000, ann()), Ok(2));
    assert_eq!(ask(&mut w, a, 1_000, ann()), Ok(3));
    assert_eq!(w.c.desk().last_refund_no(), 3);
    w.restart();
    assert_eq!(w.c.desk().last_refund_no(), 3);
    assert_eq!(ask(&mut w, b, 1_000, ann()), Ok(4));
}

#[test]
fn cr_61_a_name_or_a_reason_that_is_not_a_label_is_refused() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    w.m.reset();
    for name in ["", "a\nb", &"n".repeat(65)] {
        let actor = Actor {
            role: Role::Cashier,
            name: name.to_string(),
        };
        let err = ask(&mut w, i, 1_000, actor.clone()).unwrap_err();
        assert_eq!(err, RefundRefused::BadText("name"), "{name:?}");
        assert_eq!(err.rule(), "CR-61");
        assert_eq!(
            ask_cash(&mut w, i, 10, actor.clone()).unwrap_err(),
            RefundRefused::BadText("name")
        );
        assert_eq!(
            withdraw(&mut w, i, 1, actor).unwrap_err(),
            WithdrawRefused::BadName
        );
    }
    for reason in ["", "bell\u{7}", &"r".repeat(65)] {
        let mut req = request(1_000, ann());
        req.reason = reason.to_string();
        let err =
            w.c.request_refund(i, req, w.now, &w.chain, &mut w.m)
                .unwrap_err();
        assert_eq!(err, RefundRefused::BadText("reason"), "{reason:?}");
    }
    assert!(w.m.log.is_empty(), "nothing was written");
}

#[test]
fn cr_61_an_amount_in_the_shops_currency_is_turned_into_sats_at_its_rate_rounded_down() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    // 333 cents at $100,000 per bitcoin is 3,330 sats; at a rate of
    // 30,000,000 cents per bitcoin it is 1,110 sats exactly, and 1,111 cents
    // is 3,703.33.. sats, which is rounded down.
    let req = RefundRequest {
        ask: RefundAsk::Value {
            minor: 1_111,
            rate: crate::Rate::new(30_000_000).unwrap(),
            source: fresh(w.now),
        },
        destination: dest(),
        actor: ann(),
        reason: "wrong size".to_string(),
    };
    w.c.request_refund(i, req, w.now, &w.chain, &mut w.m)
        .unwrap();
    match &refund_events(&w)[0] {
        Event::RefundRequested { sats, ask, .. } => {
            assert_eq!(*sats, 3_703);
            assert!(matches!(
                ask,
                Ask::Value {
                    minor: 1_111,
                    minor_per_btc: 30_000_000,
                    ..
                }
            ));
        }
        e => panic!("{e:?}"),
    }
    let views = w.c.refund_views(i);
    assert_eq!(views[0].asked, 3_703);
    assert_eq!(views[0].value_minor, Some(1_111));
}

#[test]
fn cr_61_a_stale_rate_is_refused() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    let req = RefundRequest {
        ask: RefundAsk::Value {
            minor: 100,
            rate: rate(),
            source: fresh(w.now - 601),
        },
        destination: dest(),
        actor: ann(),
        reason: "wrong size".to_string(),
    };
    let err =
        w.c.request_refund(i, req, w.now, &w.chain, &mut w.m)
            .unwrap_err();
    assert_eq!(
        err,
        RefundRefused::StaleRate {
            fetched_at: w.now - 601,
            now: w.now
        }
    );
    assert_eq!(err.rule(), "CR-58");
    assert!(refund_events(&w).is_empty());
}

// CR-62.

fn p2wpkh_address() -> String {
    let script = ScriptBuf::new_p2wpkh(&bitcoin::WPubkeyHash::from_byte_array([5; 20]));
    Address::from_script(&script, Network::Bitcoin)
        .unwrap()
        .to_string()
}

fn expected_script() -> ScriptBuf {
    ScriptBuf::new_p2wpkh(&bitcoin::WPubkeyHash::from_byte_array([5; 20]))
}

#[test]
fn cr_62_a_bare_address_and_a_uri_give_the_address() {
    let a = p2wpkh_address();
    let want = expected_script();
    assert_eq!(refund_destination(&a, Network::Bitcoin), Ok(want.clone()));
    assert_eq!(
        refund_destination(&format!("bitcoin:{a}"), Network::Bitcoin),
        Ok(want.clone())
    );
    assert_eq!(
        refund_destination(&format!("BITCOIN:{a}"), Network::Bitcoin),
        Ok(want.clone())
    );
    // Surrounding white space from a scanner is trimmed; an upper-case QR
    // address is the same address.
    assert_eq!(
        refund_destination(&format!(" {}\n", a.to_uppercase()), Network::Bitcoin),
        Ok(want)
    );
}

#[test]
fn cr_62_amount_label_and_every_other_field_are_ignored_including_req_fields() {
    let a = p2wpkh_address();
    let want = expected_script();
    for query in [
        "amount=1000",
        "amount=0.00000001&label=Shop",
        "message=hi%20there",
        "req-pj=https%3A%2F%2Fexample.org",
        "req-unknown=1&amount=9999999",
        "lightning=lnbc1&unknown=x&amount=abc",
        "amount=1&amount=2",
        "&&==",
        "",
    ] {
        let uri = format!("bitcoin:{a}?{query}");
        assert_eq!(
            refund_destination(&uri, Network::Bitcoin),
            Ok(want.clone()),
            "{uri}"
        );
    }
}

#[test]
fn cr_62_a_code_with_no_readable_address_is_refused() {
    let a = p2wpkh_address();
    let refused = |text: &str| refund_destination(text, Network::Bitcoin).unwrap_err();
    for text in [
        "",
        "bitcoin:",
        "bitcoin:?amount=1",
        "hello",
        "bitcoin:hello?x=1",
    ] {
        let e = refused(text);
        assert_eq!(e, DestinationRefused::NotAnAddress, "{text:?}");
        assert_eq!(e.rule(), "CR-62");
    }
    // A query without a scheme is not a URI, so it is not an address.
    assert_eq!(
        refused(&format!("{a}?amount=1")),
        DestinationRefused::NotAnAddress
    );
    for text in [
        format!("bitcoin:{a} ?amount=1"),
        format!("bitcoin:{a}\u{0}"),
        format!("bitcoin:{a}?label=caf\u{e9}"),
    ] {
        let e = refused(&text);
        assert_eq!(e, DestinationRefused::NotPrintable, "{text:?}");
        assert_eq!(e.rule(), "CR-62");
    }
    let long = format!("bitcoin:{a}?label={}", "x".repeat(2_048));
    let e = refused(&long);
    assert_eq!(e, DestinationRefused::TooLong);
    assert_eq!(e.rule(), "CR-62");
}

// CR-63.

#[test]
fn cr_63_an_address_for_another_network_is_refused() {
    let a = p2wpkh_address();
    for network in [Network::Testnet, Network::Signet, Network::Regtest] {
        let e = refund_destination(&a, network).unwrap_err();
        assert_eq!(e, DestinationRefused::WrongNetwork, "{network:?}");
        assert_eq!(e.rule(), "CR-63");
    }
    let script = expected_script();
    let regtest = Address::from_script(&script, Network::Regtest)
        .unwrap()
        .to_string();
    assert!(refund_destination(&regtest, Network::Regtest).is_ok());
    assert_eq!(
        refund_destination(&format!("bitcoin:{regtest}?amount=1"), Network::Bitcoin),
        Err(DestinationRefused::WrongNetwork)
    );
}

#[test]
fn cr_63_a_script_the_signer_would_not_pay_is_refused() {
    use bitcoin::key::{PublicKey, XOnlyPublicKey};
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    // The five kinds the signer pays are taken.
    let secp = Secp256k1::new();
    let sk = SecretKey::from_slice(&[7; 32]).unwrap();
    let pk = PublicKey::new(sk.public_key(&secp));
    let (xonly, _) = XOnlyPublicKey::from_keypair(&sk.keypair(&secp));
    let kinds = [
        ScriptBuf::new_p2pkh(&pk.pubkey_hash()),
        ScriptBuf::new_p2sh(&ScriptBuf::new_p2pkh(&pk.pubkey_hash()).script_hash()),
        ScriptBuf::new_p2wpkh(&pk.wpubkey_hash().unwrap()),
        p2wsh(9),
        ScriptBuf::new_p2tr(&secp, xonly, None),
    ];
    for script in &kinds {
        let a = Address::from_script(script, Network::Bitcoin)
            .unwrap()
            .to_string();
        assert_eq!(
            refund_destination(&a, Network::Bitcoin).as_ref(),
            Ok(script),
            "{a}"
        );
    }
    // A witness program of a version that is not 0 or 1, a pay-to-anchor
    // output and a bare script have no such address, or have one the signer
    // would not pay.
    let v2 = ScriptBuf::new_witness_program(
        &bitcoin::WitnessProgram::new(bitcoin::WitnessVersion::V2, &[3u8; 32]).unwrap(),
    );
    let anchor = ScriptBuf::new_witness_program(
        &bitcoin::WitnessProgram::new(bitcoin::WitnessVersion::V1, &[0x4e, 0x73]).unwrap(),
    );
    for script in [v2, anchor] {
        let a = Address::from_script(&script, Network::Bitcoin)
            .unwrap()
            .to_string();
        let e = refund_destination(&a, Network::Bitcoin).unwrap_err();
        assert_eq!(e, DestinationRefused::ScriptKind, "{a}");
        assert_eq!(e.rule(), "CR-63");
    }
}

#[test]
fn cr_63_a_destination_script_that_is_not_a_payment_script_is_refused_by_the_call() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    for script in [
        ScriptBuf::new(),
        ScriptBuf::from_bytes(vec![0x6a, 0x01, 0x00]),
        ScriptBuf::from_bytes(vec![0x51, 0x20, 1, 2]),
    ] {
        let mut req = request(1_000, ann());
        req.destination = script;
        let err =
            w.c.request_refund(i, req, w.now, &w.chain, &mut w.m)
                .unwrap_err();
        assert_eq!(err, RefundRefused::DestinationKind);
        assert_eq!(err.rule(), "CR-63");
    }
    assert!(refund_events(&w).is_empty());
}

// CR-64.

#[test]
fn cr_64_a_request_for_an_unknown_sale_is_refused() {
    let mut w = World::new();
    final_sale(&mut w);
    w.m.reset();
    let err = ask(&mut w, 41, 1_000, ann()).unwrap_err();
    assert_eq!(err, RefundRefused::UnknownSale(41));
    assert_eq!(err.rule(), "CR-64");
    assert_eq!(
        ask_cash(&mut w, 41, 10, ann()).unwrap_err(),
        RefundRefused::UnknownSale(41)
    );
    let err = withdraw(&mut w, 41, 1, ann()).unwrap_err();
    assert_eq!(err, WithdrawRefused::UnknownSale(41));
    assert_eq!(err.rule(), "CR-64");
    assert!(w.m.log.is_empty());
    assert!(w.c.refund_views(41).is_empty());
}

// CR-65.

#[test]
fn cr_65_a_sale_with_nothing_received_is_refused() {
    let mut w = World::new();
    let o = w.open_five();
    let err = ask(&mut w, o.index, 1_000, ann()).unwrap_err();
    assert_eq!(err, RefundRefused::NothingReceived);
    assert_eq!(err.rule(), "CR-65");
    assert_eq!(
        ask_cash(&mut w, o.index, 10, ann()).unwrap_err(),
        RefundRefused::NothingReceived
    );
    // An expired sale and a cancelled one hold nothing either.
    w.tick(1_000);
    w.judge(o.index);
    assert_eq!(
        ask(&mut w, o.index, 1_000, ann()).unwrap_err(),
        RefundRefused::NothingReceived
    );
    let c = w.open_five();
    w.c.cancel(c.index, w.now, &mut w.m).unwrap();
    assert_eq!(
        ask(&mut w, c.index, 1_000, ann()).unwrap_err(),
        RefundRefused::NothingReceived
    );
    // Dust is nothing.
    let d = w.open_five();
    let t = w.pay(&d, 500);
    w.see(&t, 1);
    w.judge(d.index);
    assert_eq!(
        ask(&mut w, d.index, 500, ann()).unwrap_err(),
        RefundRefused::NothingReceived
    );
    assert!(refund_events(&w).is_empty());
}

// CR-66.

#[test]
fn cr_66_a_sale_below_the_refund_depth_is_recorded_as_waiting_for_depth() {
    let mut w = World::new();
    // One confirmation: the shop's depth, not the refund depth of 6.
    let i = paid_sale(&mut w, 500, 5_000, 100);
    assert_eq!(w.judge(i).state, crate::State::Confirmed);
    let number = ask(&mut w, i, 1_000, ann()).unwrap();
    match &refund_events(&w)[0] {
        Event::RefundRequested {
            waiting_for_depth, ..
        } => assert!(*waiting_for_depth),
        e => panic!("{e:?}"),
    }
    let views = w.c.refund_views(i);
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].state, RefundState::WaitingForDepth);
    assert_eq!(views[0].number, Some(number));
    assert!(views[0].state.name().contains("waiting for depth"));
}

#[test]
fn cr_66_the_waiting_request_becomes_requested_when_the_depth_is_reached() {
    let mut w = World::new();
    let i = paid_sale(&mut w, 500, 5_000, 100);
    ask(&mut w, i, 1_000, ann()).unwrap();
    assert_eq!(w.c.refund_views(i)[0].state, RefundState::WaitingForDepth);
    // Five more blocks: six confirmations. The flag is worked out again
    // from the sale, and the stored state is the same entry.
    w.chain.tip = 105;
    assert_eq!(w.judge(i).state, crate::State::Final);
    let views = w.c.refund_views(i);
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].state, RefundState::Requested);
    let before = w.m.records.len();
    w.restart();
    assert_eq!(w.c.refund_views(i)[0].state, RefundState::Requested);
    assert_eq!(w.m.records.len(), before, "no new record for the change");
}

#[test]
fn cr_66_a_reorg_that_undoes_the_payment_puts_the_flag_back() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    let block = w.confirm(&[t], 95);
    assert_eq!(w.judge(o.index).state, crate::State::Final);
    ask(&mut w, o.index, 1_000, ann()).unwrap();
    assert_eq!(w.c.refund_views(o.index)[0].state, RefundState::Requested);
    w.orphan(&block);
    w.judge(o.index);
    assert_eq!(
        w.c.refund_views(o.index)[0].state,
        RefundState::WaitingForDepth
    );
}

#[test]
fn cr_66_a_request_after_a_reorg_took_a_final_sale_below_the_depth_is_waiting() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    let block = w.confirm(&[t], 95);
    assert_eq!(w.judge(o.index).state, crate::State::Final);
    w.orphan(&block);
    // The sale had been final once; now no block holds the payment. The
    // flag is worked out from the sale as it is, not from the earlier state.
    let number = ask(&mut w, o.index, 1_000, ann()).unwrap();
    match refund_events(&w).last().unwrap() {
        Event::RefundRequested {
            number: n,
            waiting_for_depth,
            ..
        } => assert_eq!((*n, *waiting_for_depth), (number, true)),
        e => panic!("{e:?}"),
    }
    // The screen's state follows the next judgment of the sale.
    w.judge(o.index);
    assert_eq!(
        w.c.refund_views(o.index)[0].state,
        RefundState::WaitingForDepth
    );
}

// CR-67.

#[test]
fn cr_67_two_requests_that_together_pass_what_was_received_make_the_second_refused() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    ask(&mut w, i, 3_000, ann()).unwrap();
    w.m.reset();
    let err = ask(&mut w, i, 2_001, ann()).unwrap_err();
    assert_eq!(
        err,
        RefundRefused::MoreThanReceived {
            received: 5_000,
            asked: 3_000,
            sats: 2_001
        }
    );
    assert_eq!(err.rule(), "CR-67");
    assert!(w.m.log.is_empty(), "nothing was written");
    assert_eq!(w.c.desk().last_refund_no(), 1, "no number was used");
    // What is left can be asked, to the satoshi.
    assert_eq!(ask(&mut w, i, 2_000, ann()), Ok(2));
    assert!(matches!(
        ask(&mut w, i, 1_000, ann()).unwrap_err(),
        RefundRefused::MoreThanReceived { .. }
    ));
}

#[test]
fn cr_67_a_zero_amount_and_one_below_the_destinations_dust_limit_are_refused() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    let err = ask(&mut w, i, 0, ann()).unwrap_err();
    assert_eq!(err, RefundRefused::ZeroAmount);
    assert_eq!(err.rule(), "CR-67");
    // P2WSH: 330.
    let err = ask(&mut w, i, 329, ann()).unwrap_err();
    assert_eq!(
        err,
        RefundRefused::BelowDust {
            sats: 329,
            dust: 330
        }
    );
    assert_eq!(err.rule(), "CR-67");
    assert!(ask(&mut w, i, 330, ann()).is_ok());
    // A value that comes to nothing at its rate is zero too.
    let req = RefundRequest {
        ask: RefundAsk::Value {
            minor: 0,
            rate: rate(),
            source: fresh(w.now),
        },
        destination: dest(),
        actor: ann(),
        reason: "wrong size".to_string(),
    };
    assert_eq!(
        w.c.request_refund(i, req, w.now, &w.chain, &mut w.m)
            .unwrap_err(),
        RefundRefused::ZeroAmount
    );
    let req = RefundRequest {
        ask: RefundAsk::Value {
            minor: u64::MAX,
            rate: crate::Rate::new(1).unwrap(),
            source: fresh(w.now),
        },
        destination: dest(),
        actor: max(),
        reason: "wrong size".to_string(),
    };
    assert_eq!(
        w.c.request_refund(i, req, w.now, &w.chain, &mut w.m)
            .unwrap_err(),
        RefundRefused::AmountTooLarge
    );
    assert_eq!(
        ask(&mut w, i, u64::MAX, max()).unwrap_err(),
        RefundRefused::AmountTooLarge
    );
}

#[test]
fn cr_67_a_cash_refund_and_a_request_that_together_pass_what_was_received_are_refused() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    // 300 cents is 3,000 sats at the test rate.
    ask_cash(&mut w, i, 300, ann()).unwrap();
    let err = ask(&mut w, i, 2_001, ann()).unwrap_err();
    assert_eq!(
        err,
        RefundRefused::MoreThanReceived {
            received: 5_000,
            asked: 3_000,
            sats: 2_001
        }
    );
    assert!(ask(&mut w, i, 2_000, ann()).is_ok());
    // And the other way round: nothing is left for the till.
    let err = ask_cash(&mut w, i, 1, ann()).unwrap_err();
    assert!(matches!(err, RefundRefused::MoreThanReceived { .. }));
    assert_eq!(err.rule(), "CR-67");
}

#[test]
fn cr_67_a_withdrawn_request_no_longer_counts_as_asked() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    ask(&mut w, i, 5_000, ann()).unwrap();
    assert!(ask(&mut w, i, 330, ann()).is_err());
    withdraw(&mut w, i, 1, ann()).unwrap();
    assert_eq!(ask(&mut w, i, 5_000, ann()), Ok(2));
}

#[test]
fn cr_67_a_sale_keeps_at_most_thirty_two_refund_entries() {
    let mut w = World::new();
    let i = paid_sale(&mut w, 5_000, 50_000, 95);
    for n in 1..=32u64 {
        assert_eq!(ask(&mut w, i, 330, max()), Ok(n));
    }
    let err = ask(&mut w, i, 330, max()).unwrap_err();
    assert_eq!(err, RefundRefused::TooManyRefunds);
    assert_eq!(err.rule(), "CR-67");
    // The entries and the numbers read back after a restart.
    w.restart();
    assert_eq!(w.c.refund_views(i).len(), 32);
}

// CR-68.

#[test]
fn cr_68_a_destination_in_the_counters_own_batches_is_refused() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    // The address the sale itself was paid to, one that was never shown, and
    // the last of the batch.
    let own = [w.c.sale(i).unwrap().script().clone(), p2wsh(50), p2wsh(100)];
    for script in own {
        let mut req = request(1_000, ann());
        req.destination = script;
        let err =
            w.c.request_refund(i, req, w.now, &w.chain, &mut w.m)
                .unwrap_err();
        assert_eq!(err, RefundRefused::OwnScript);
        assert_eq!(err.rule(), "CR-68");
    }
    let mut req = request(1_000, ann());
    req.destination = p2wsh(101);
    assert!(w
        .c
        .request_refund(i, req, w.now, &w.chain, &mut w.m)
        .is_ok());
    assert_eq!(refund_events(&w).len(), 1);
}

// CR-69.

/// A $1,000 sale paid in full (1,000,000 sats), at the refund depth. The
/// settings allow a cashier 100,000 sats for one refund and 300,000 sats a
/// day.
fn big_sale(w: &mut World) -> u32 {
    paid_sale(w, 100_000, 1_000_000, 95)
}

#[test]
fn cr_69_a_cashier_over_the_single_refund_limit_needs_a_manager() {
    let mut w = World::new();
    let i = big_sale(&mut w);
    let err = ask(&mut w, i, 100_001, ann()).unwrap_err();
    assert_eq!(
        err,
        RefundRefused::NeedsManagerSingle {
            sats: 100_001,
            limit: 100_000
        }
    );
    assert_eq!(err.rule(), "CR-69");
    assert!(err.to_string().contains("manager is needed"));
    assert_eq!(ask(&mut w, i, 100_000, ann()), Ok(1));
    // A manager is not held to it.
    assert_eq!(ask(&mut w, i, 400_000, max()), Ok(2));
    assert_eq!(
        ask_cash(&mut w, i, 10_001, ann()).unwrap_err().rule(),
        "CR-69",
        "a cash refund is held to it as well"
    );
}

#[test]
fn cr_69_a_cashier_over_the_daily_cap_needs_a_manager() {
    let mut w = World::new();
    let i = big_sale(&mut w);
    for n in 1..=3 {
        assert_eq!(ask(&mut w, i, 90_000, ann()), Ok(n));
    }
    // 270,000 so far; another 40,000 passes 300,000.
    let err = ask(&mut w, i, 40_000, ann()).unwrap_err();
    assert_eq!(
        err,
        RefundRefused::NeedsManagerDaily {
            sats: 40_000,
            already: 270_000,
            cap: 300_000
        }
    );
    assert_eq!(err.rule(), "CR-69");
    // Another cashier has a cap of their own.
    let bob = Actor {
        role: Role::Cashier,
        name: "bob".to_string(),
    };
    assert_eq!(ask(&mut w, i, 40_000, bob), Ok(4));
    // A manager goes on.
    assert_eq!(ask(&mut w, i, 40_000, max()), Ok(5));
    // The window moves on: a day and a day of blocks later the cap is free
    // again (CR-120).
    w.tick(86_401);
    w.chain.tip += 144;
    assert_eq!(ask(&mut w, i, 40_000, ann()), Ok(6));
}

#[test]
fn cr_69_withdrawn_requests_and_cash_refunds_count_for_the_cap() {
    let mut w = World::new();
    let i = big_sale(&mut w);
    // 90,000 sats withdrawn, 90,000 in cash (900 cents), 90,000 requested.
    ask(&mut w, i, 90_000, ann()).unwrap();
    withdraw(&mut w, i, 1, ann()).unwrap();
    ask_cash(&mut w, i, 9_000, ann()).unwrap();
    ask(&mut w, i, 90_000, ann()).unwrap();
    let err = ask(&mut w, i, 40_000, ann()).unwrap_err();
    assert_eq!(
        err,
        RefundRefused::NeedsManagerDaily {
            sats: 40_000,
            already: 270_000,
            cap: 300_000
        }
    );
    // The cap is also kept across a restart: the entries are in the sales.
    w.restart();
    assert!(matches!(
        ask(&mut w, i, 40_000, ann()).unwrap_err(),
        RefundRefused::NeedsManagerDaily {
            already: 270_000,
            ..
        }
    ));
}

// CR-70.

#[test]
fn cr_70_a_request_can_be_withdrawn_and_asked_again() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    assert_eq!(ask(&mut w, i, 5_000, ann()), Ok(1));
    w.m.reset();
    w.tick(60);
    withdraw(&mut w, i, 1, max()).unwrap();
    // The record, then the sale.
    assert_eq!(w.m.log, ["record", "Sale(0)"]);
    assert_eq!(
        refund_events(&w).last().unwrap(),
        &Event::RefundWithdrawn {
            index: i,
            at: w.now,
            number: 1,
            role: Role::Manager,
            name: "max".to_string(),
        }
    );
    assert_eq!(w.c.refund_views(i)[0].state, RefundState::Withdrawn);
    // Asked again: a new number, the whole amount.
    assert_eq!(ask(&mut w, i, 5_000, ann()), Ok(2));
    let states: Vec<RefundState> = w.c.refund_views(i).iter().map(|v| v.state).collect();
    assert_eq!(states, [RefundState::Withdrawn, RefundState::Requested]);
}

#[test]
fn cr_70_a_request_that_does_not_exist_or_was_withdrawn_cannot_be_withdrawn() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    let j = final_sale(&mut w);
    ask(&mut w, i, 1_000, ann()).unwrap();
    let err = withdraw(&mut w, i, 7, ann()).unwrap_err();
    assert_eq!(err, WithdrawRefused::UnknownRequest(7));
    assert_eq!(err.rule(), "CR-70");
    // The request is of another sale.
    assert_eq!(
        withdraw(&mut w, j, 1, ann()).unwrap_err(),
        WithdrawRefused::UnknownRequest(1)
    );
    withdraw(&mut w, i, 1, ann()).unwrap();
    let err = withdraw(&mut w, i, 1, ann()).unwrap_err();
    assert_eq!(err, WithdrawRefused::AlreadyWithdrawn(1));
    assert_eq!(err.rule(), "CR-70");
    // A cash refund has no number and cannot be withdrawn.
    ask_cash(&mut w, i, 10, ann()).unwrap();
    assert_eq!(
        withdraw(&mut w, i, 0, ann()).unwrap_err(),
        WithdrawRefused::UnknownRequest(0)
    );
    assert_eq!(
        refund_events(&w)
            .iter()
            .filter(|e| matches!(e, Event::RefundWithdrawn { .. }))
            .count(),
        1
    );
}

// CR-71.

#[test]
fn cr_71_a_cash_refund_is_a_record_valued_at_the_sales_last_quote() {
    let mut w = World::new();
    let o = w.open_five();
    // A new quote at $50,000 per bitcoin: the unpaid part is priced at it.
    w.requote(o.index, 5_000_000).unwrap();
    let t = w.pay(&o, 5_000);
    w.confirm(&[t], 95);
    w.judge(o.index);
    w.tick(10);
    w.m.reset();
    ask_cash(&mut w, o.index, 123, ann()).unwrap();
    assert_eq!(w.m.log, ["record", "Sale(0)"]);
    // 123 cents at 5,000,000 cents per bitcoin: 2,460 sats exactly; one
    // more cent rounds up.
    assert_eq!(
        refund_events(&w).last().unwrap(),
        &Event::RefundPaidInCash {
            index: o.index,
            at: w.now,
            role: Role::Cashier,
            name: "ann".to_string(),
            reason: "till refund".to_string(),
            minor: 123,
            sats: 2_460,
            chain_height: Some(100),
        }
    );
    ask_cash(&mut w, o.index, 1, ann()).unwrap();
    match refund_events(&w).last().unwrap() {
        Event::RefundPaidInCash { sats, .. } => assert_eq!(*sats, 20),
        e => panic!("{e:?}"),
    }
    let views = w.c.refund_views(o.index);
    assert_eq!(views[0].state, RefundState::PaidInCash);
    assert_eq!(views[0].number, None);
    assert_eq!(views[0].value_minor, Some(123));
    assert_eq!(views[0].asked, 2_460);
}

#[test]
fn cr_71_a_cash_refund_asks_the_owner_for_nothing_and_does_not_take_a_number() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    w.m.reset();
    ask_cash(&mut w, i, 100, ann()).unwrap();
    assert_eq!(w.c.desk().last_refund_no(), 0);
    assert!(!w.m.log.contains(&"Desk".to_string()));
    // The only record is the cash one: no request.
    assert!(refund_events(&w)
        .iter()
        .all(|e| matches!(e, Event::RefundPaidInCash { .. })));
    assert_eq!(ask(&mut w, i, 1_000, ann()), Ok(1));
}

#[test]
fn cr_71_a_cash_refund_of_nothing_is_refused() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    let err = ask_cash(&mut w, i, 0, ann()).unwrap_err();
    assert_eq!(err, RefundRefused::ZeroAmount);
    assert_eq!(err.rule(), "CR-67");
}

// CR-72.

#[test]
fn cr_72_the_views_say_what_was_asked_by_whom_and_when() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    ask(&mut w, i, 1_000, ann()).unwrap();
    w.tick(5);
    ask(&mut w, i, 2_000, max()).unwrap();
    w.tick(5);
    ask_cash(&mut w, i, 50, ann()).unwrap();
    withdraw(&mut w, i, 1, ann()).unwrap();
    let views = w.c.refund_views(i);
    assert_eq!(views.len(), 3);
    assert_eq!(
        (
            views[0].sale,
            views[0].number,
            views[0].asked,
            views[0].state
        ),
        (i, Some(1), 1_000, RefundState::Withdrawn)
    );
    assert_eq!(views[0].by, ann());
    assert_eq!(views[0].at, T0);
    assert_eq!(
        (
            views[1].number,
            views[1].asked,
            views[1].state,
            &views[1].by
        ),
        (Some(2), 2_000, RefundState::Requested, &max())
    );
    assert_eq!(views[1].at, T0 + 5);
    assert_eq!(views[2].state, RefundState::PaidInCash);
    assert_eq!(views[2].at, T0 + 10);
    // A sale without refunds has none.
    let other = final_sale(&mut w);
    assert!(w.c.refund_views(other).is_empty());
    // They read back after a restart.
    w.restart();
    assert_eq!(w.c.refund_views(i), views);
}

// CR-73.

#[test]
fn cr_73_the_writes_are_the_desk_the_record_then_the_sale() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    w.m.reset();
    ask(&mut w, i, 1_000, ann()).unwrap();
    assert_eq!(w.m.log, ["Desk", "record", "Sale(0)"]);
}

#[test]
fn cr_73_a_failed_desk_write_changes_nothing() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    w.m.reset();
    w.m.fail_on = Some(1);
    let err = ask(&mut w, i, 1_000, ann()).unwrap_err();
    assert!(matches!(err, RefundRefused::Store(_)));
    assert_eq!(err.rule(), "CR-40");
    assert!(refund_events(&w).is_empty());
    assert!(w.c.refund_views(i).is_empty());
    w.m.reset();
    assert_eq!(
        ask(&mut w, i, 1_000, ann()),
        Ok(1),
        "the number was not used"
    );
}

#[test]
fn cr_73_a_stop_after_the_desk_wastes_a_number_and_never_reuses_one() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    w.m.reset();
    w.m.fail_on = Some(2); // the record
    assert!(matches!(
        ask(&mut w, i, 1_000, ann()).unwrap_err(),
        RefundRefused::Store(_)
    ));
    assert!(refund_events(&w).is_empty());
    assert!(w.c.refund_views(i).is_empty());
    // Even after a restart the number is gone.
    w.m.reset();
    w.restart();
    assert_eq!(w.c.desk().last_refund_no(), 1);
    assert_eq!(ask(&mut w, i, 1_000, ann()), Ok(2));
}

/// The refund requests the counter's records hold, as `(number, sats)`.
fn recorded_requests(w: &World) -> Vec<(u64, u64)> {
    refund_events(w)
        .into_iter()
        .filter_map(|e| match e {
            Event::RefundRequested { number, sats, .. } => Some((number, sats)),
            _ => None,
        })
        .collect()
}

/// What the views of a sale say, as `(number, asked, state)`.
fn viewed(w: &World, index: u32) -> Vec<(Option<u64>, u64, RefundState)> {
    w.c.refund_views(index)
        .into_iter()
        .map(|v| (v.number, v.asked, v.state))
        .collect()
}

#[test]
fn cr_73_a_stop_after_each_write_of_a_request_leaves_the_sale_equal_to_the_records() {
    for stop in 1..=3usize {
        let mut w = World::new();
        let i = final_sale(&mut w);
        w.m.reset();
        w.m.fail_on = Some(stop); // 1 the desk, 2 the record, 3 the sale
        assert!(matches!(
            ask(&mut w, i, 2_000, ann()).unwrap_err(),
            RefundRefused::Store(_)
        ));
        let held = usize::from(stop == 3);
        // (a), (b): as soon as the call failed, and again after a restart,
        // the views are what the records say.
        for restarted in [false, true] {
            if restarted {
                w.m.reset();
                w.restart();
            }
            assert_eq!(recorded_requests(&w).len(), held, "stop {stop}");
            let views = viewed(&w, i);
            assert_eq!(views.len(), held, "stop {stop} restarted {restarted}");
            if held == 1 {
                assert_eq!(views[0], (Some(1), 2_000, RefundState::Requested));
            }
        }
        // (c): the room left counts the adopted request. 3,001 more is over
        // what was received only when the 2,000 is held.
        w.m.reset();
        let more = ask(&mut w, i, 3_001, ann());
        if held == 1 {
            assert_eq!(
                more.unwrap_err(),
                RefundRefused::MoreThanReceived {
                    received: 5_000,
                    asked: 2_000,
                    sats: 3_001
                },
                "stop {stop}"
            );
            assert_eq!(ask(&mut w, i, 3_000, ann()), Ok(2));
        } else {
            // A number may skip: after a stop following the desk it does.
            assert_eq!(more, Ok(if stop == 1 { 1 } else { 2 }), "stop {stop}");
        }
        // The stored sale agrees with the records after the next call.
        w.restart();
        let numbers: Vec<_> = viewed(&w, i).into_iter().map(|v| v.0).collect();
        let recorded: Vec<_> = recorded_requests(&w)
            .into_iter()
            .map(|(n, _)| Some(n))
            .collect();
        assert_eq!(numbers, recorded, "stop {stop}");
    }
}

#[test]
fn cr_73_one_ask_that_stopped_after_the_record_is_one_record_and_one_refund() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    w.m.reset();
    w.m.fail_on = Some(3);
    assert!(ask(&mut w, i, 4_000, ann()).is_err());
    w.m.reset();
    w.restart();
    // The cashier reads the views, sees the request, and does not ask again.
    // If the same ask is made again it is the same room: 4,000 and 4,000 is
    // more than 5,000 was received.
    assert_eq!(
        ask(&mut w, i, 4_000, ann()).unwrap_err().rule(),
        "CR-67",
        "the second ask is refused"
    );
    assert_eq!(recorded_requests(&w), vec![(1, 4_000)]);
    assert_eq!(w.c.refund_views(i).len(), 1);
}

#[test]
fn cr_73_a_sale_that_cannot_be_stored_again_stops_the_next_call_before_it_acts() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    w.m.reset();
    w.m.fail_on = Some(3);
    assert!(ask(&mut w, i, 1_000, ann()).is_err());
    w.m.reset();
    w.restart();
    // The first write of the next call is the sale adopted from the record.
    w.m.fail_on = Some(1);
    let before = recorded_requests(&w).len();
    assert!(matches!(
        ask(&mut w, i, 1_000, ann()).unwrap_err(),
        RefundRefused::Store(_)
    ));
    assert_eq!(recorded_requests(&w).len(), before, "nothing was done");
    assert_eq!(w.c.desk().last_refund_no(), 1);
    w.m.reset();
    assert_eq!(ask(&mut w, i, 1_000, ann()), Ok(2));
    w.restart();
    assert_eq!(w.c.refund_views(i).len(), 2);
}

#[test]
fn cr_73_a_stop_after_each_write_of_a_withdrawal_leaves_the_sale_equal_to_the_records() {
    for stop in 1..=2usize {
        let mut w = World::new();
        let i = final_sale(&mut w);
        ask(&mut w, i, 4_000, ann()).unwrap();
        w.m.reset();
        w.m.fail_on = Some(stop); // 1 the record, 2 the sale
        assert!(withdraw(&mut w, i, 1, ann()).is_err());
        let withdrawn = stop == 2;
        let want = if withdrawn {
            RefundState::Withdrawn
        } else {
            RefundState::Requested
        };
        for restarted in [false, true] {
            if restarted {
                w.m.reset();
                w.restart();
            }
            assert_eq!(viewed(&w, i), vec![(Some(1), 4_000, want)], "stop {stop}");
            let records = refund_events(&w)
                .iter()
                .filter(|e| matches!(e, Event::RefundWithdrawn { .. }))
                .count();
            assert_eq!(records, usize::from(withdrawn));
        }
        w.m.reset();
        if withdrawn {
            // The owner and the counter agree: it is withdrawn, and the
            // room it held is free again.
            assert_eq!(
                withdraw(&mut w, i, 1, ann()).unwrap_err(),
                WithdrawRefused::AlreadyWithdrawn(1)
            );
            assert_eq!(ask(&mut w, i, 5_000, ann()), Ok(2));
        } else {
            assert_eq!(ask(&mut w, i, 2_000, ann()).unwrap_err().rule(), "CR-67");
            withdraw(&mut w, i, 1, ann()).unwrap();
        }
    }
}

#[test]
fn cr_73_a_stop_after_each_write_of_a_cash_refund_leaves_the_sale_equal_to_the_records() {
    for stop in 1..=2usize {
        let mut w = World::new();
        let i = big_sale(&mut w);
        w.m.reset();
        w.m.fail_on = Some(stop); // 1 the record, 2 the sale
                                  // 9,000 cents are 90,000 satoshis at ten satoshis a cent.
        assert!(ask_cash(&mut w, i, 9_000, ann()).is_err());
        let held = usize::from(stop == 2);
        for restarted in [false, true] {
            if restarted {
                w.m.reset();
                w.restart();
            }
            let cash = refund_events(&w)
                .iter()
                .filter(|e| matches!(e, Event::RefundPaidInCash { .. }))
                .count();
            assert_eq!(cash, held, "stop {stop}");
            assert_eq!(w.c.refund_views(i).len(), held);
            if held == 1 {
                assert_eq!(w.c.refund_views(i)[0].state, RefundState::PaidInCash);
            }
        }
        // The cashier's daily cap of 300,000 counts the cash refund: two
        // requests of 90,000 bring it to 270,000 only when it is held.
        w.m.reset();
        ask(&mut w, i, 90_000, ann()).unwrap();
        ask(&mut w, i, 90_000, ann()).unwrap();
        let third = ask(&mut w, i, 40_000, ann());
        if held == 1 {
            assert_eq!(third.unwrap_err().rule(), "CR-69");
        } else {
            assert!(third.is_ok());
        }
        // Never more than received counts it too.
        w.m.reset();
        let rest = ask(&mut w, i, 1_000_000, max()).unwrap_err();
        assert_eq!(rest.rule(), "CR-67");
    }
}

#[test]
fn cr_73_the_desks_last_number_is_never_below_the_highest_record() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    ask(&mut w, i, 1_000, ann()).unwrap();
    // An older desk than the records: the number is taken from the records.
    w.m.slots.insert(Slot::Desk, crate::Desk::new().to_bytes());
    w.restart();
    assert_eq!(w.c.desk().last_refund_no(), 1);
    w.m.reset();
    assert_eq!(ask(&mut w, i, 1_000, ann()), Ok(2));
}

#[test]
fn cr_73_the_sale_with_its_entries_is_stored_with_the_sale() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    ask(&mut w, i, 1_000, ann()).unwrap();
    let stored = &w.m.slots[&Slot::Sale(i)];
    assert_eq!(stored[4], 2, "version 2 holds the refund entries");
    // A sale without a refund is still the old version 1.
    let j = final_sale(&mut w);
    assert_eq!(w.m.slots[&Slot::Sale(j)][4], 1);
}

// CR-119.

#[test]
fn cr_119_a_cashier_cannot_give_cash_on_top_of_a_withdrawn_request() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    // 5,000 sats arrived; ann asks for 4,000 and withdraws it. In cash 300
    // cents are 3,000 sats, so 7,000 of 5,000 if the withdrawn request counts.
    ask(&mut w, i, 4_000, ann()).unwrap();
    withdraw(&mut w, i, 1, ann()).unwrap();
    let err = ask_cash(&mut w, i, 300, ann()).unwrap_err();
    assert_eq!(
        err,
        RefundRefused::WithdrawnMayBePaid {
            received: 5_000,
            asked: 4_000,
            sats: 3_000
        }
    );
    assert_eq!(err.rule(), "CR-119");
    assert!(err.to_string().starts_with("CR-119: "));
    // Nothing was written for the refused call.
    assert_eq!(w.c.refund_views(i).len(), 1);
    // A cash refund that fits beside the withdrawn request is allowed.
    assert!(ask_cash(&mut w, i, 40, ann()).is_ok());
}

#[test]
fn cr_119_a_manager_gives_cash_when_the_request_was_withdrawn() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    ask(&mut w, i, 4_000, ann()).unwrap();
    withdraw(&mut w, i, 1, ann()).unwrap();
    assert!(ask_cash(&mut w, i, 300, max()).is_ok());
    // And more cash than is left, counting only what is not withdrawn, is
    // refused as before.
    let err = ask_cash(&mut w, i, 250, max()).unwrap_err();
    assert!(matches!(err, RefundRefused::MoreThanReceived { .. }));
}

#[test]
fn cr_119_a_new_request_ignores_a_withdrawn_one() {
    let mut w = World::new();
    let i = final_sale(&mut w);
    ask(&mut w, i, 4_000, ann()).unwrap();
    withdraw(&mut w, i, 1, ann()).unwrap();
    assert_eq!(ask(&mut w, i, 4_000, ann()), Ok(2));
    // And a cashier's request that is over what arrived is refused with
    // the CR-67 words, not these.
    let err = ask(&mut w, i, 4_000, ann()).unwrap_err();
    assert!(matches!(err, RefundRefused::MoreThanReceived { .. }));
}

// CR-120.

/// Ann asks three refunds of 90,000 sats at the start; 270,000 of the
/// 300,000 sats cap is used.
fn ann_near_the_cap(w: &mut World) -> u32 {
    let i = big_sale(w);
    for n in 1..=3 {
        assert_eq!(ask(w, i, 90_000, ann()), Ok(n));
    }
    i
}

fn over_the_cap(w: &mut World, i: u32) -> bool {
    matches!(
        ask(w, i, 40_000, ann()),
        Err(RefundRefused::NeedsManagerDaily { .. })
    )
}

#[test]
fn cr_120_an_entry_from_the_last_day_by_the_clock_counts() {
    let mut w = World::new();
    let i = ann_near_the_cap(&mut w);
    // The chain moved on a great deal; the clock did not.
    w.tick(3_600);
    w.chain.tip += 500;
    assert!(over_the_cap(&mut w, i));
    // The clock past the day and the chain too: free.
    w.tick(86_400);
    assert!(!over_the_cap(&mut w, i));
}

#[test]
fn cr_120_an_entry_fewer_than_144_blocks_old_counts_whatever_the_clock_says() {
    let mut w = World::new();
    let i = ann_near_the_cap(&mut w);
    // The date was set two days forward.
    w.tick(2 * 86_400);
    w.chain.tip += 143;
    assert!(over_the_cap(&mut w, i));
    // The entries are 144 blocks old now.
    w.chain.tip += 1;
    assert!(!over_the_cap(&mut w, i));
    // The heights come back from the stored sale after a restart.
    let mut w = World::new();
    let i = ann_near_the_cap(&mut w);
    w.restart();
    w.tick(2 * 86_400);
    assert!(over_the_cap(&mut w, i));
}

#[test]
fn cr_120_an_entry_timed_after_the_clock_counts() {
    let mut w = World::new();
    let i = ann_near_the_cap(&mut w);
    // The date was set back, and the chain has moved on.
    w.now -= 3 * 86_400;
    w.chain.tip += 1_000;
    assert!(over_the_cap(&mut w, i));
}

#[test]
fn cr_120_an_entry_without_a_height_counts_by_the_clock_alone() {
    use crate::refund::{EntryKind, RefundEntry};
    let entry = |height| RefundEntry {
        kind: EntryKind::Cash,
        number: 0,
        at: 1_000,
        role: Role::Cashier,
        name: "ann".to_string(),
        sats: 10,
        value_minor: Some(1),
        withdrawn: false,
        height,
    };
    let old = 1_000 + 86_400;
    assert!(!entry(None).counts_for_cap(old, Some(500)));
    assert!(!entry(Some(100)).counts_for_cap(old, None));
    assert!(entry(Some(100)).counts_for_cap(old, Some(243)));
    assert!(!entry(Some(100)).counts_for_cap(old, Some(244)));
    assert!(entry(None).counts_for_cap(old - 1, None));
    assert!(entry(None).counts_for_cap(999, None));
}

#[test]
fn cr_120_entries_and_records_carry_the_height_and_read_back() {
    use crate::refund::{EntryKind, RefundEntry};
    let entry = |kind, height| RefundEntry {
        kind,
        number: u64::from(kind == EntryKind::Request),
        at: 1_000,
        role: Role::Cashier,
        name: "ann".to_string(),
        sats: 10,
        value_minor: Some(1),
        withdrawn: false,
        height,
    };
    for kind in [EntryKind::Request, EntryKind::Cash] {
        let plain = entry(kind, None);
        let mut a = Vec::new();
        plain.write(&mut a);
        // Without a height the bytes are the earlier build's.
        assert_eq!(a[0], if kind == EntryKind::Request { 1 } else { 2 });
        let with = entry(kind, Some(812_345));
        let mut b = Vec::new();
        with.write(&mut b);
        assert_eq!(b[0], if kind == EntryKind::Request { 3 } else { 4 });
        assert_eq!(b.len(), a.len() + 4);
        for (bytes, want) in [(a, plain), (b, with)] {
            let mut r = openbmt_core::wire::Reader::new(&bytes);
            assert_eq!(RefundEntry::read(&mut r).unwrap(), want);
            r.finish().unwrap();
        }
    }
    // A height of zero is not a height.
    let mut zero = Vec::new();
    entry(EntryKind::Cash, Some(1)).write(&mut zero);
    let n = zero.len();
    zero[n - 4..].copy_from_slice(&0u32.to_le_bytes());
    assert!(RefundEntry::read(&mut openbmt_core::wire::Reader::new(&zero)).is_err());
    // A record without a height keeps its earlier tag; with one it takes
    // the new tag and four bytes more.
    let plain = Event::RefundRequested {
        index: 1,
        at: 1_000,
        number: 1,
        role: Role::Cashier,
        name: "ann".to_string(),
        reason: "x".to_string(),
        destination: dest(),
        received_sats: 5_000,
        waiting_for_depth: false,
        sats: 10,
        ask: Ask::Sats,
        chain_height: None,
    };
    let mut with = plain.clone();
    if let Event::RefundRequested { chain_height, .. } = &mut with {
        *chain_height = Some(7);
    }
    let (a, b) = (plain.encode(), with.encode());
    assert_eq!((a[0], b[0]), (16, 22));
    assert_eq!(b.len(), a.len() + 4);
    assert_eq!(Event::decode(&b).unwrap(), with);
    // The new tag without its height, or with a zero one, is refused.
    let mut short = b.clone();
    short.truncate(a.len());
    assert!(Event::decode(&short).is_err());
    let mut zero = b;
    let n = zero.len();
    zero[n - 4..].copy_from_slice(&0u32.to_le_bytes());
    assert!(Event::decode(&zero).is_err());
    // The earlier tag with four bytes more is not read.
    let mut long = a;
    long.extend_from_slice(&7u32.to_le_bytes());
    assert!(Event::decode(&long).is_err());
}

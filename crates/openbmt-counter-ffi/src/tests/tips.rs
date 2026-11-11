//! CR-187 to CR-192 through `dispatch`: the settings view carries the tips,
//! `tip_choices` and `open_with_tip` take the tip with the sale, a tip above
//! the sale is refused with its words, and the day's totals and the refund
//! rows say how much is tip.

use bitcoin::{Address, Network};
use openbmt_core::settings::SignedSettings;

use super::kit::*;
use super::wire::{Rd, Report, W};

/// The test settings with tips on and these percents.
fn tipped(percents: &[u8]) -> SignedSettings {
    let mut s = signed_settings().settings;
    s.tips_on = true;
    s.tip_percents = percents.to_vec();
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    }
}

/// A started node whose settings are `settings`, that has heard from the
/// network.
fn ready_with(settings: SignedSettings) -> (Rig, u64) {
    let rig = Rig::new();
    let dir = rig.dir();
    let mut config = start_config(dir.to_str().expect("UTF-8"));
    config.record = record_with(settings).to_bytes();
    let handle = rig.start_with(&config);
    rig.send(vec![peers_event()]);
    let response = rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    let mut r = Rd::ok(&response);
    r.pump();
    r.done();
    (rig, handle)
}

fn choices(rig: &Rig, handle: u64, amount: u64) -> Vec<(u8, u64)> {
    let response = rig.call(59, handle, &W::new().u64(amount).0);
    let mut r = Rd::ok(&response);
    let list = r.list(|r| (r.u8(), r.u64()));
    r.done();
    list
}

/// The fields of `open_with_tip`, with no person at the till.
fn tip_fields(amount: u64, tip: u64) -> Vec<u8> {
    W::new()
        .u64(T0)
        .u64(amount)
        .u64(tip)
        .string("USD")
        .u64(RATE)
        .string("test-fixed")
        .u64(T0)
        .u8(0)
        .0
}

#[test]
fn cr_187_the_settings_view_carries_the_tips() {
    let rig = Rig::new();
    for (settings, on, percents) in [
        (signed_settings(), false, vec![10, 15, 20]),
        (tipped(&[5, 18]), true, vec![5, 18]),
    ] {
        let dir = rig.dir();
        let response = rig.call(
            43,
            0,
            &W::new()
                .bytes(&record_with(settings).to_bytes())
                .string(dir.to_str().expect("UTF-8"))
                .0,
        );
        let mut r = Rd::ok(&response);
        r.u8();
        r.u32();
        r.u8();
        r.u8();
        r.u64();
        assert_eq!(r.string(), "USD");
        assert_eq!((r.bool(), r.bytes()), (on, percents));
    }
}

#[test]
fn cr_188_the_tips_offered_are_the_settings_percents_of_the_amount() {
    let (rig, handle) = ready_with(tipped(&[10, 15, 20]));
    // 12.50: 1.25 (half up to 1.25), 1.875 to 1.88, 2.50.
    assert_eq!(
        choices(&rig, handle, 1_250),
        vec![(10, 125), (15, 188), (20, 250)]
    );
    let (rig, handle) = ready_with(signed_settings());
    assert!(
        choices(&rig, handle, 1_250).is_empty(),
        "tips off offers none"
    );
}

#[test]
fn cr_189_a_sale_with_a_tip_is_for_the_amount_and_the_tip_together() {
    let (rig, handle) = ready_with(tipped(&[10, 15, 20]));
    let response = rig.call(60, handle, &tip_fields(500, 100));
    let mut r = Rd::ok(&response);
    let opened = r.opened();
    r.done();
    // 6.00 at 1,000 sats a cent-unit: the code is for 6,000 sats.
    assert_eq!(opened.needs_sats, 6_000);
    // A tip of nothing is the sale of `open`.
    let response = rig.call(60, handle, &tip_fields(500, 0));
    let mut r = Rd::ok(&response);
    assert_eq!(r.opened().needs_sats, 5_000);
    r.done();
}

#[test]
fn cr_190_a_tip_above_the_sale_is_refused_with_words_and_opens_nothing() {
    let (rig, handle) = ready_with(tipped(&[10, 15, 20]));
    let f = Rd::error(&rig.call(60, handle, &tip_fields(500, 501))).expect("refused");
    assert_eq!((f.status, f.rule.as_str()), (1, "CR-190"));
    assert_eq!(f.words.heading, "Tip not accepted");
    assert!(!f.words.next.is_empty());
    // The tip equal to the sale is allowed; nothing was opened by the refusal.
    let response = rig.call(60, handle, &tip_fields(500, 500));
    let mut r = Rd::ok(&response);
    assert_eq!(r.opened().index, 0);
    r.done();
}

#[test]
fn cr_189_the_tip_ops_refuse_every_cut_of_a_request() {
    let (rig, handle) = ready_with(tipped(&[10, 15, 20]));
    for (op, body) in [(59u16, W::new().u64(500).0), (60, tip_fields(500, 100))] {
        for n in 0..body.len() {
            let response = rig.call(op, handle, &body[..n]);
            assert!(Rd::error(&response).is_some(), "op {op} cut at {n}");
        }
        let mut longer = body.clone();
        longer.push(0);
        assert!(Rd::error(&rig.call(op, handle, &longer)).is_some());
    }
    // No node, no sale.
    assert!(Rd::error(&rig.call(60, 0, &tip_fields(500, 100))).is_some());
}

#[test]
fn cr_191_cr_192_a_paid_tip_is_in_the_totals_and_in_the_refund_rows() {
    let (rig, handle) = ready_with(tipped(&[10, 15, 20]));
    let name = "ann";
    let fields = W::new()
        .u64(T0)
        .u64(500)
        .u64(100)
        .string("USD")
        .u64(RATE)
        .string("test-fixed")
        .u64(T0)
        .u8(1)
        .actor(false, name)
        .0;
    let response = rig.call(60, handle, &fields);
    let mut r = Rd::ok(&response);
    let opened = r.opened();
    r.done();
    let pay = spend(1, &[(&script_of(opened.index), 6_000)]);
    rig.send(vec![sighting(&pay, vec![peer(20)])]);
    rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    let block = mine(genesis(), 1, std::slice::from_ref(&pay));
    rig.send(vec![joined(1, &block), block_event(&block)]);
    let response = rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    let mut r = Rd::ok(&response);
    let (reports, _, _) = r.pump();
    r.done();
    assert!(reports.contains(&Report::PosPaid(opened.index)));

    let response = rig.call(58, handle, &W::new().u64(T0 - 10).u64(T0 + 86_400).u8(1).0);
    let mut r = Rd::ok(&response);
    let day = r.day_totals();
    r.done();
    assert_eq!(day.tips, (1, 100));
    // The sales' own amounts are without the tip.
    assert_eq!(day.paid.1, 500);
    assert_eq!(day.people[0].tips_minor, 100);

    // A refund of 5.50 is the whole sale and 0.50 of the tip.
    let destination = Address::from_script(&p2wsh(0x77), Network::Regtest)
        .expect("an address")
        .to_string();
    let ask = W::new()
        .u64(T0)
        .u32(opened.index)
        .u8(1)
        .u64(550)
        .u64(RATE)
        .string("test-fixed")
        .u64(T0)
        .string(&destination)
        .actor(true, "mia")
        .string("changed mind")
        .0;
    Rd::ok(&rig.call(16, handle, &ask)).u64();
    let response = rig.call(19, handle, &W::new().u32(opened.index).0);
    let mut r = Rd::ok(&response);
    let rows = r.list(|r| {
        r.u32();
        r.opt(Rd::u64);
        r.u64();
        r.opt(Rd::u64);
        let tip = r.u64();
        r.u8();
        r.u8();
        r.string();
        r.u64();
        r.words();
        tip
    });
    r.done();
    assert_eq!(rows, vec![50]);
}

/// The counter app's words of the tip step, in `TipWords.kt`, are the
/// library's (CR-188, CR-191, CR-192).
#[test]
fn cr_188_the_app_words_of_the_tips_are_the_librarys() {
    use openbmt_counter::say::{
        tip_button_label, tip_choose_words, tips_person_label, TIPS_LINE_LABEL, TIP_NONE_LABEL,
        TIP_OTHER_LABEL,
    };
    let path = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("../../apps/counter-android/src/model/TipWords.kt");
    let kotlin = std::fs::read_to_string(&path).expect("TipWords.kt");
    let words = tip_choose_words("$amount");
    for text in [
        words.heading.as_str(),
        words.what.as_str(),
        words.next.as_str(),
        words.code.as_deref().expect("a code"),
        TIPS_LINE_LABEL,
        TIP_OTHER_LABEL,
        TIP_NONE_LABEL,
    ] {
        assert!(kotlin.contains(&format!("\"{text}\"")), "{text}");
    }
    // The templates take the same text as the library's functions make.
    assert_eq!(tips_person_label("$name"), "Tips taken by $name");
    assert!(kotlin.contains("\"Tips taken by $name\""));
    assert_eq!(tip_button_label(7, "$tip"), "7% \u{b7} $tip");
    assert!(kotlin.contains("\"$percent% \\u00B7 $tip\""));
}

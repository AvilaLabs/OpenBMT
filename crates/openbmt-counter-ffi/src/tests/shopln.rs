//! SL-72, SL-76, SL-41 through `dispatch`: the Lightning counter asks for
//! its seed, a seed of another node is refused with words and nothing stored,
//! and a seed of the pinned node comes back as the record for the app to seal.

use openbmt_core::settings::{Settings, ShopLightning, SignedSettings};
use openbmt_core::shop_lightning::node_id;

use super::kit::*;
use super::wire::{Rd, W};
use crate::StartConfig;

const OP_SEED: u16 = 90;
const OP_STATE: u16 = 91;

fn seed(n: u8) -> [u8; 64] {
    std::array::from_fn(|i| n.wrapping_add(i as u8))
}

fn hex(seed: &[u8; 64]) -> String {
    seed.iter().map(|b| format!("{b:02x}")).collect()
}

fn settings_naming(counter: u8) -> SignedSettings {
    let mut s: Settings = signed_settings().settings;
    s.devices.insert(id(61).device_id());
    s.boxes.insert(id(61).device_id());
    s.devices.insert(id(counter).device_id());
    s.counters.entry(id(counter).device_id()).or_insert(0);
    s.shop_lightning = Some(ShopLightning {
        counter: id(counter).device_id(),
        node_id: node_id(&seed(1)),
        second_device: id(61).device_id(),
        provider_node_id: node_id(&seed(99)),
        provider_addr: "127.0.0.1:9".parse().expect("address"),
        cap_sats: 100_000,
        max_opening_fee_sats: 3_000,
    });
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    }
}

fn config(rig: &Rig, counter: u8) -> StartConfig {
    StartConfig {
        dir: rig.dir().to_str().expect("UTF-8").to_string(),
        now: T0,
        record: record_with(settings_naming(counter)).to_bytes(),
        proxy: None,
        own_node: None,
        lightning: None,
        link_proxy: None,
        shop_seed: None,
    }
}

/// (needs_seed, delete_seed, reason heading, scan heading).
fn state(rig: &Rig, handle: u64) -> (bool, bool, Option<String>, String) {
    let response = rig.call(OP_STATE, handle, &[]);
    let mut r = Rd::ok(&response);
    let out = (
        r.bool(),
        r.bool(),
        r.opt(Rd::words).map(|w| w.heading),
        r.words().heading,
    );
    r.done();
    out
}

#[test]
fn sl_76_the_lightning_counter_with_no_seed_asks_for_the_key() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, 3));
    let (needs, delete, why, scan) = state(&rig, handle);
    assert!(needs && !delete);
    assert!(why.expect("the reason").starts_with("WORDS_NEEDED_SL"));
    assert!(scan.starts_with("WORDS_NEEDED_SL76_"));
}

#[test]
fn sl_76_a_counter_that_is_not_named_never_asks_for_the_key() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, 4));
    let (needs, delete, why, _) = state(&rig, handle);
    assert!(!needs && !delete && why.is_none());
}

#[test]
fn sl_72_a_seed_of_another_id_is_refused_and_the_pinned_one_comes_back_as_a_record() {
    let rig = Rig::new();
    let handle = rig.start_with(&config(&rig, 3));
    let wrong = rig.call(OP_SEED, handle, &W::new().string(&hex(&seed(2))).0);
    let fault = Rd::error(&wrong).expect("refused");
    assert_eq!(fault.rule, "SL-72");
    assert!(fault.words.heading.starts_with("WORDS_NEEDED_SL72_"));
    let right = rig.call(OP_SEED, handle, &W::new().string(&hex(&seed(1))).0);
    let mut r = Rd::ok(&right);
    let record = r.list(Rd::u8);
    r.done();
    assert!(!record.is_empty());
    let text = hex(&seed(1));
    assert!(
        !record.windows(text.len()).any(|w| w == text.as_bytes()),
        "no plain hex in the record"
    );
}

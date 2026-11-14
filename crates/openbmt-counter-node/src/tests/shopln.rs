//! Shop Lightning at the counter (SL-70 to SL-78) on a fake driver: which
//! counter runs it, the seed, the reasons a sale shows the on-chain code
//! alone, and the words. The node itself and the Box are in
//! `scripts/shop-lightning-counter-e2e-core.sh`.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use openbmt_core::identity::DeviceId;
use openbmt_core::settings::{Settings, ShopLightning};
use openbmt_core::shop_lightning::node_id;
use openbmt_counter::Amount;

use super::kit::*;
use crate::say::{all_shopln_words, is_placeholder, shopln_words};
use crate::shopln::{SeedNotTaken, SeedRecord, Why};
use crate::{NodeConfig, SaleLightning, ShopLnConfig};

fn box_id() -> DeviceId {
    id(61).device_id()
}

fn seed(n: u8) -> [u8; 64] {
    std::array::from_fn(|i| n.wrapping_add(i as u8))
}

fn hex(seed: &[u8; 64]) -> String {
    seed.iter().map(|b| format!("{b:02x}")).collect()
}

/// Settings that name counter `counter` as the Lightning counter, with the
/// node of seed `seed_n`.
fn settings_naming(counter: u8, seed_n: u8) -> Settings {
    let mut s = settings();
    s.devices.insert(box_id());
    s.boxes.insert(box_id());
    s.devices.insert(id(counter).device_id());
    s.counters.entry(id(counter).device_id()).or_insert(10_000);
    s.shop_lightning = Some(ShopLightning {
        counter: id(counter).device_id(),
        node_id: node_id(&seed(seed_n)),
        second_device: box_id(),
        provider_node_id: node_id(&seed(99)),
        provider_addr: "127.0.0.1:9".parse().unwrap(),
        cap_sats: 100_000,
        max_opening_fee_sats: 3_000,
    });
    s
}

fn config_with(
    dir: &Scratch,
    time: &Arc<AtomicU64>,
    settings: Settings,
    seed_record: Option<Vec<u8>>,
) -> NodeConfig {
    let mut c = config(dir.path(), time);
    c.settings = settings;
    c.shop_lightning = Some(ShopLnConfig {
        seed_record,
        identity: Box::new(|| Some(id(3))),
        relay: None,
        link_proxy: None,
    });
    c
}

fn rig(dir: &Scratch, settings: Settings, seed_record: Option<Vec<u8>>) -> Rig {
    let time = Arc::new(AtomicU64::new(T0));
    let mut rig = start_with(config_with(dir, &time, settings, seed_record), time);
    assert!(rig.feed(vec![Rig::peers()]).is_empty());
    rig
}

fn sale(rig: &mut Rig) -> u32 {
    rig.node
        .open(Amount(500), usd(), rate(), &source_at(&rig.time))
        .unwrap()
        .index
}

fn files_of(dir: &Scratch) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.path().join("shop-lightning"))
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn sl_70_a_counter_the_settings_do_not_name_takes_on_chain_only_even_with_the_field_set() {
    let dir = Scratch::new();
    // The field names counter 4; this is counter 3.
    let mut rig = rig(&dir, settings_naming(4, 1), None);
    let index = sale(&mut rig);
    assert_eq!(rig.node.lightning_sale(index), SaleLightning::Off);
    rig.feed(vec![Rig::peers()]);
    assert_eq!(rig.node.lightning_sale(index), SaleLightning::Off);
    let status = rig.node.shop_lightning_status().unwrap();
    assert!(!status.needs_seed && status.why.is_none() && !status.delete_seed);
    // A seed given to it is refused, for the pinned ID is of another counter.
    assert_eq!(
        rig.node.shop_lightning_take_seed(&hex(&seed(1))),
        Err(SeedNotTaken::NotHere)
    );
    assert!(rig.node.shop_lightning_report().is_none());
    assert!(files_of(&dir).is_empty());
}

#[test]
fn sl_70_a_shop_without_the_field_has_no_shop_lightning_on_any_counter() {
    let dir = Scratch::new();
    let mut rig = rig(&dir, settings(), None);
    let index = sale(&mut rig);
    assert_eq!(rig.node.lightning_sale(index), SaleLightning::Off);
    assert!(rig.node.shop_lightning_report().is_none());
    // And a counter the app gave nothing for has none to ask.
    let dir2 = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut plain = start_with(config(dir2.path(), &time), time);
    assert!(plain.node.shop_lightning_status().is_none());
    assert_eq!(
        plain.node.shop_lightning_take_seed(&hex(&seed(1))),
        Err(SeedNotTaken::NotHere)
    );
}

#[test]
fn sl_76_the_lightning_counter_without_a_seed_asks_for_it_and_offers_no_invoice() {
    let dir = Scratch::new();
    let mut rig = rig(&dir, settings_naming(3, 1), None);
    let index = sale(&mut rig);
    rig.feed(vec![Rig::peers()]);
    let SaleLightning::Unavailable(words) = rig.node.lightning_sale(index) else {
        panic!("the sale shows the reason");
    };
    assert_eq!(words.heading, "WORDS_NEEDED_SL76_NO_SEED");
    assert!(is_placeholder(&words));
    let status = rig.node.shop_lightning_status().unwrap();
    assert!(status.needs_seed);
    assert_eq!(status.why, Some(Why::NoSeed));
}

#[test]
fn sl_72_a_seed_of_another_id_is_refused_with_words_and_nothing_is_stored() {
    let dir = Scratch::new();
    let mut rig = rig(&dir, settings_naming(3, 1), None);
    let e = rig
        .node
        .shop_lightning_take_seed(&hex(&seed(2)))
        .unwrap_err();
    assert!(matches!(e, SeedNotTaken::WrongNode(_)));
    let words = e.words();
    assert_eq!(words.heading, "WORDS_NEEDED_SL72_WRONG_NODE");
    assert_eq!(words.code, Some("SL-72"));
    // Nothing was kept: still asking for the key, and no file made.
    let status = rig.node.shop_lightning_status().unwrap();
    assert!(status.needs_seed);
    assert!(files_of(&dir).is_empty());
    // A text that is not a key is refused too.
    assert_eq!(
        rig.node.shop_lightning_take_seed("not a key"),
        Err(SeedNotTaken::NotASeed)
    );
    assert!(rig.node.shop_lightning_status().unwrap().needs_seed);
}

#[test]
fn sl_72_a_seed_of_the_pinned_id_is_given_back_as_a_record_for_the_app_to_seal() {
    let dir = Scratch::new();
    let mut rig = rig(&dir, settings_naming(3, 1), None);
    let bytes = rig.node.shop_lightning_take_seed(&hex(&seed(1))).unwrap();
    let record = SeedRecord::from_bytes(&bytes).unwrap();
    assert_eq!(record.seed(), &seed(1));
    // It does not go into a file of the counter's folder: the app seals it.
    let all = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect::<Vec<_>>();
    for p in all {
        assert!(!std::fs::read(&p).unwrap().windows(64).any(|w| w == seed(1)));
    }
    // A second one is not taken while this one is held.
    assert_eq!(
        rig.node.shop_lightning_take_seed(&hex(&seed(1))),
        Err(SeedNotTaken::AlreadyStored)
    );
    // The key is held, so the screen is not asked for again.
    assert!(!rig.node.shop_lightning_status().unwrap().needs_seed);
}

#[test]
fn sl_33_a_counter_with_the_key_and_no_wallet_description_starts_nothing_and_says_why() {
    let dir = Scratch::new();
    let record = SeedRecord::new(seed(1)).to_bytes();
    let mut rig = rig(&dir, settings_naming(3, 1), Some(record));
    let index = sale(&mut rig);
    rig.feed(vec![Rig::peers()]);
    // A counter paired with the Key app holds no description of the wallet,
    // so the sweep has nowhere to go (SL-33).
    let SaleLightning::Unavailable(words) = rig.node.lightning_sale(index) else {
        panic!("the sale shows the reason");
    };
    assert_eq!(words.heading, "WORDS_NEEDED_SL33_NO_WALLET");
    assert_eq!(
        rig.node.shop_lightning_status().unwrap().why,
        Some(Why::NoWallet)
    );
    // The seed was read from the app's record and not asked for again.
    assert!(!rig.node.shop_lightning_status().unwrap().needs_seed);
}

#[test]
fn sl_60_the_counter_does_not_start_the_node_on_the_real_network() {
    let dir = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut c = config_with(
        &dir,
        &time,
        settings_naming(3, 1),
        Some(SeedRecord::new(seed(1)).to_bytes()),
    );
    c.driver.network = bitcoin::Network::Bitcoin;
    let mut rig = start_with(c, time);
    rig.feed(vec![Rig::peers()]);
    let status = rig.node.shop_lightning_status().unwrap();
    assert_eq!(status.why, Some(Why::Mainnet));
    // No folder of the node's was made.
    assert!(!dir.path().join("shop-lightning").join("node").exists());
}

#[test]
fn sl_77_a_seed_with_no_node_behind_it_is_deleted_when_the_settings_do_not_name_it() {
    let dir = Scratch::new();
    let record = SeedRecord::new(seed(1)).to_bytes();
    let mut rig = rig(&dir, settings(), Some(record));
    rig.feed(vec![Rig::peers()]);
    let status = rig.node.shop_lightning_status().unwrap();
    assert!(status.delete_seed, "the app deletes what it sealed");
    assert!(!status.needs_seed);
}

#[test]
fn sl_77_a_plan_whose_seed_is_gone_is_forgotten() {
    let dir = Scratch::new();
    let sl = settings_naming(3, 1).shop_lightning.unwrap();
    crate::shopln::Book {
        plan: Some(sl),
        ..Default::default()
    }
    .save(&dir.path().join("shop-lightning"))
    .unwrap();
    let mut rig = rig(&dir, settings(), None);
    rig.feed(vec![Rig::peers()]);
    let book = crate::shopln::Book::load(&dir.path().join("shop-lightning")).unwrap();
    assert_eq!(book.plan, None);
}

#[test]
fn sl_41_every_reason_has_words_that_are_placeholders_with_their_own_name() {
    let all = all_shopln_words();
    let mut seen = std::collections::BTreeSet::new();
    for (name, words) in &all {
        assert!(is_placeholder(words), "{name}");
        assert_eq!(words.heading, words.what, "{name}");
        assert_eq!(words.heading, words.next, "{name}");
        assert!(seen.insert(words.heading.clone()), "{name} repeats");
        assert!(words.heading.starts_with("WORDS_NEEDED_SL"), "{name}");
    }
    // The node's own text is for the small print.
    let w = shopln_words(&Why::Node(
        openbmt_lightning_node::room::OnChainReason::SecondDeviceAway,
    ));
    assert!(w.detail.unwrap().contains("not answering"));
}

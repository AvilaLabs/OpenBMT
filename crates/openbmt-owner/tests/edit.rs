//! Changing the settings: an edit applied to the settings held, sorted as
//! the queue sorts it, and nothing stored by the preview (AS-74 to AS-77).

mod common;

use common::*;
use openbmt_core::settings::{Change, FieldChange, Limit, Settings};
use openbmt_owner::{
    parse_lightning_node, sats_per_usd_at, LightningEdit, NodeKeyFault, Refusal, SettingsEdit,
    SettingsQueued, WatchEdit, MAX_USD_PER_BTC, MIN_USD_PER_BTC,
};

/// $100,000 a bitcoin: 1,000 satoshis to a dollar, the rate of the rig's settings.
const PRICE: u64 = 100_000;

/// The generator point, as a compressed key: a real point on the curve.
const KEY: &str = "0279BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798";

/// A rig whose settings in force are `settings_with(1, [1, 2])` after `base`.
fn held(base: impl Fn(&mut Settings)) -> Rig {
    let mut rig = Rig::new();
    let mut s = settings_with(1, &[1, 2]);
    base(&mut s);
    ok(rig.owner.queue_settings(s, nop));
    rig.round(&[], 0);
    assert_eq!(rig.owner.known_settings().unwrap().version, 1);
    rig
}

fn plain(_: &mut Settings) {}

struct Case {
    name: &'static str,
    base: fn(&mut Settings),
    edit: fn(&mut SettingsEdit),
    direction: Change,
}

fn node_key() -> [u8; 33] {
    parse_lightning_node(KEY).unwrap()
}

/// A loosening edit and a tightening edit of every field.
fn cases() -> Vec<Case> {
    use Change::{Loosening as L, Tightening as T};
    macro_rules! case {
        ($name:expr, $base:expr, $edit:expr, $dir:expr) => {
            Case {
                name: $name,
                base: $base,
                edit: $edit,
                direction: $dir,
            }
        };
    }
    vec![
        case!("light_per_sale", plain, |e| e.light_per_sale = Some(11), L),
        case!("light_per_sale", plain, |e| e.light_per_sale = Some(9), T),
        case!("light_per_sale", plain, |e| e.light_per_sale = Some(0), T),
        case!("light_budget", plain, |e| e.light_budget = Some(51), L),
        case!("light_budget", plain, |e| e.light_budget = Some(49), T),
        case!("node_per_sale", plain, |e| e.node_per_sale = Some(26), L),
        case!("node_per_sale", plain, |e| e.node_per_sale = Some(24), T),
        case!("node_budget", plain, |e| e.node_budget = Some(101), L),
        case!("node_budget", plain, |e| e.node_budget = Some(99), T),
        case!("box_per_sale", plain, |e| e.box_per_sale = Some(11), L),
        case!("box_per_sale", plain, |e| e.box_per_sale = Some(9), T),
        case!("box_budget", plain, |e| e.box_budget = Some(51), L),
        case!("box_budget", plain, |e| e.box_budget = Some(49), T),
        case!(
            "underpayment_tolerance_bps",
            plain,
            |e| e.underpayment_tolerance_bps = Some(51),
            L
        ),
        case!(
            "underpayment_tolerance_bps",
            plain,
            |e| e.underpayment_tolerance_bps = Some(49),
            T
        ),
        case!("refund_cap", plain, |e| e.refund_cap = Some(101), L),
        case!("refund_cap", plain, |e| e.refund_cap = Some(99), T),
        case!(
            "refund_daily_cap",
            plain,
            |e| e.refund_daily_cap = Some(301),
            L
        ),
        case!(
            "refund_daily_cap",
            plain,
            |e| e.refund_daily_cap = Some(299),
            T
        ),
        case!(
            "cashier_override_daily_cap",
            plain,
            |e| e.cashier_override_daily_cap = Some(21),
            L
        ),
        case!(
            "cashier_override_daily_cap",
            plain,
            |e| e.cashier_override_daily_cap = Some(19),
            T
        ),
        case!(
            "manager_override_ceiling",
            plain,
            |e| e.manager_override_ceiling = Some(101),
            L
        ),
        case!(
            "manager_override_ceiling",
            plain,
            |e| e.manager_override_ceiling = Some(99),
            T
        ),
        case!(
            "min_confirmations",
            |s| s.min_confirmations = 3,
            |e| e.min_confirmations = Some(2),
            L
        ),
        case!(
            "min_confirmations",
            plain,
            |e| e.min_confirmations = Some(2),
            T
        ),
        case!(
            "final_confirmations",
            plain,
            |e| e.final_confirmations = Some(5),
            L
        ),
        case!(
            "final_confirmations",
            plain,
            |e| e.final_confirmations = Some(7),
            T
        ),
        case!(
            "heartbeat_max_age_secs",
            plain,
            |e| e.heartbeat_max_age_secs = Some(601),
            L
        ),
        case!(
            "heartbeat_max_age_secs",
            plain,
            |e| e.heartbeat_max_age_secs = Some(599),
            T
        ),
        case!("tor_on", |s| s.tor_on = true, |e| e.tor_on = Some(false), L),
        case!("tor_on", plain, |e| e.tor_on = Some(true), T),
        case!("customer_wifi", plain, |e| e.customer_wifi = Some(false), L),
        case!(
            "customer_wifi",
            |s| s.customer_wifi = false,
            |e| e.customer_wifi = Some(true),
            T
        ),
        case!(
            "min_sale_sats",
            |s| s.min_sale_sats = 1_000,
            |e| e.min_sale_sats = Some(500),
            L
        ),
        case!(
            "min_sale_sats",
            |s| s.min_sale_sats = 1_000,
            |e| e.min_sale_sats = Some(0),
            L
        ),
        case!(
            "min_sale_sats",
            |s| s.min_sale_sats = 1_000,
            |e| e.min_sale_sats = Some(2_000),
            T
        ),
        case!(
            "counters",
            |s| {
                for share in s.counters.values_mut() {
                    *share = 4_000;
                }
            },
            |e| e.shares = vec![(counter(1).device_id(), 5_000)],
            L
        ),
        case!(
            "counters",
            plain,
            |e| e.shares = vec![(counter(1).device_id(), 4_000)],
            T
        ),
        case!(
            "lightning_node",
            plain,
            |e| e.lightning_node = Some(LightningEdit::Set(KEY.to_string())),
            L
        ),
        case!(
            "lightning_node",
            |s| s.lightning_node = Some(node_key()),
            |e| e.lightning_node = Some(LightningEdit::Remove),
            T
        ),
    ]
}

#[test]
fn as_74_every_field_has_an_edit_that_loosens_and_one_that_tightens() {
    let mut seen = std::collections::BTreeSet::new();
    for case in cases() {
        let mut rig = held(case.base);
        let mut edit = SettingsEdit {
            usd_per_btc: Some(PRICE),
            ..SettingsEdit::default()
        };
        (case.edit)(&mut edit);
        let preview = rig.owner.settings_preview(&edit).expect("a preview");
        assert_eq!(
            preview.fields,
            vec![FieldChange {
                field: case.name,
                direction: case.direction
            }],
            "{}",
            case.name
        );
        assert_eq!(preview.change, case.direction, "{}", case.name);
        assert_eq!(preview.settings.version, 2);
        // AS-77: the queue sorts the same settings the same way.
        let queued = ok(rig.owner.queue_settings(preview.settings, nop));
        assert_eq!(
            matches!(queued, SettingsQueued::Tightening { .. }),
            case.direction == Change::Tightening,
            "{}",
            case.name
        );
        seen.insert((case.name, case.direction == Change::Loosening));
    }
    // Every editable field was both loosened and tightened.
    assert_eq!(seen.len(), 2 * 19);
}

#[test]
fn as_74_what_is_not_named_is_kept_and_an_empty_edit_changes_nothing() {
    let rig = held(plain);
    let preview = rig
        .owner
        .settings_preview(&SettingsEdit::default())
        .expect("a preview");
    let mut same = rig.owner.known_settings().unwrap().clone();
    same.version = 2;
    assert_eq!(preview.settings, same);
    assert!(preview.fields.is_empty());
    assert_eq!(preview.change, Change::Tightening);
}

#[test]
fn as_74_the_edit_is_applied_to_the_latest_settings_and_shown_against_those_in_force() {
    let mut rig = held(plain);
    // A queued loosening that is not approved yet.
    let loosen = SettingsEdit {
        refund_cap: Some(500),
        usd_per_btc: Some(PRICE),
        ..SettingsEdit::default()
    };
    let first = rig.owner.settings_preview(&loosen).unwrap();
    ok(rig.owner.queue_settings(first.settings, nop));
    // A tightening edit of another field starts from the queued settings,
    // so it carries the loosening with it, and the preview says so.
    let tighten = SettingsEdit {
        tor_on: Some(true),
        ..SettingsEdit::default()
    };
    let second = rig.owner.settings_preview(&tighten).unwrap();
    assert_eq!(second.settings.version, 3);
    assert_eq!(second.settings.refund_cap.display_usd, 500);
    assert_eq!(second.change, Change::Loosening);
    let names: Vec<_> = second.fields.iter().map(|f| f.field).collect();
    assert_eq!(names, ["refund_cap", "tor_on"]);
    assert_eq!(second.reference.version, 1);
}

#[test]
fn as_74_no_settings_held_or_queued_is_refused() {
    let o = new_owner(openbmt_owner::Options::default());
    let edit = SettingsEdit {
        tor_on: Some(true),
        ..SettingsEdit::default()
    };
    assert_eq!(
        o.settings_preview(&edit).unwrap_err(),
        Refusal::NoSettingsToChange
    );
}

#[test]
fn as_74_values_that_are_not_allowed_are_refused() {
    let rig = held(plain);
    let refuse = |edit: SettingsEdit| match rig.owner.settings_preview(&edit) {
        Err(Refusal::SettingsInvalid(why)) => why,
        other => panic!("{other:?}"),
    };
    for depth in [0u8, 101] {
        refuse(SettingsEdit {
            min_confirmations: Some(depth),
            ..SettingsEdit::default()
        });
        refuse(SettingsEdit {
            final_confirmations: Some(depth),
            ..SettingsEdit::default()
        });
    }
    // The final depth may not be below the confirmed depth.
    refuse(SettingsEdit {
        min_confirmations: Some(8),
        ..SettingsEdit::default()
    });
    refuse(SettingsEdit {
        underpayment_tolerance_bps: Some(501),
        ..SettingsEdit::default()
    });
    for secs in [59, 3_601] {
        refuse(SettingsEdit {
            heartbeat_max_age_secs: Some(secs),
            ..SettingsEdit::default()
        });
    }
    // The depths 1 and 100 and the tolerance 500 are allowed.
    let edge = SettingsEdit {
        min_confirmations: Some(1),
        final_confirmations: Some(100),
        underpayment_tolerance_bps: Some(500),
        ..SettingsEdit::default()
    };
    assert!(rig.owner.settings_preview(&edge).is_ok());
}

#[test]
fn as_74_shares_that_add_up_to_more_than_the_whole_are_refused() {
    let rig = held(plain);
    let (a, b) = (counter(1).device_id(), counter(2).device_id());
    let too_much = SettingsEdit {
        shares: vec![(a, 8_000), (b, 5_000)],
        ..SettingsEdit::default()
    };
    assert!(matches!(
        rig.owner.settings_preview(&too_much),
        Err(Refusal::SettingsInvalid(_))
    ));
    // Less than the whole is allowed: the rest is left unspent.
    let less = SettingsEdit {
        shares: vec![(a, 2_000), (b, 3_000)],
        ..SettingsEdit::default()
    };
    let preview = rig.owner.settings_preview(&less).unwrap();
    assert_eq!(preview.settings.counters[&a], 2_000);
    assert_eq!(preview.change, Change::Tightening);
    // A key that is not a counter has no share to change.
    let stranger = SettingsEdit {
        shares: vec![(counter(9).device_id(), 100)],
        ..SettingsEdit::default()
    };
    assert!(matches!(
        rig.owner.settings_preview(&stranger),
        Err(Refusal::SettingsInvalid(_))
    ));
}

#[test]
fn as_75_a_changed_limit_without_a_price_is_refused() {
    let rig = held(|s| {
        for l in [
            &mut s.light_per_sale,
            &mut s.light_budget,
            &mut s.node_per_sale,
            &mut s.node_budget,
            &mut s.box_per_sale,
            &mut s.box_budget,
            &mut s.refund_cap,
            &mut s.refund_daily_cap,
            &mut s.cashier_override_daily_cap,
            &mut s.manager_override_ceiling,
        ] {
            *l = Limit {
                sats: 0,
                display_usd: 0,
            };
        }
    });
    let edit = SettingsEdit {
        light_per_sale: Some(5),
        ..SettingsEdit::default()
    };
    assert_eq!(
        rig.owner.settings_preview(&edit).unwrap_err(),
        Refusal::EditNeedsRate
    );
    // Turning a limit off, or changing another field, needs no price.
    let off = SettingsEdit {
        light_per_sale: Some(0),
        tor_on: Some(true),
        ..SettingsEdit::default()
    };
    let preview = rig.owner.settings_preview(&off).unwrap();
    assert_eq!(preview.price, None);
    // With a price the limit is made at it.
    let priced = SettingsEdit {
        usd_per_btc: Some(PRICE),
        ..edit
    };
    let preview = rig.owner.settings_preview(&priced).unwrap();
    assert_eq!(preview.settings.light_per_sale, Limit::from_usd(5, 1_000));
    assert_eq!(preview.price, Some((PRICE, 1_000)));
}

#[test]
fn as_75_the_price_is_turned_into_satoshis_to_a_dollar_rounded_to_the_nearest() {
    assert_eq!(sats_per_usd_at(100_000), Ok(1_000));
    assert_eq!(sats_per_usd_at(98_500), Ok(1_015));
    assert_eq!(sats_per_usd_at(1_000), Ok(100_000));
    assert_eq!(sats_per_usd_at(10_000_000), Ok(10));
    // 100,000,000 / 3,000 is 33,333.33, / 6,000 is 16,666.67 and / 1,600,000 is 62.5: a half rounds up.
    assert_eq!(sats_per_usd_at(3_000), Ok(33_333));
    assert_eq!(sats_per_usd_at(6_000), Ok(16_667));
    assert_eq!(sats_per_usd_at(80_000), Ok(1_250));
    assert_eq!(sats_per_usd_at(1_600_000), Ok(63));
    assert_eq!(
        sats_per_usd_at(MIN_USD_PER_BTC - 1),
        Err(Refusal::PriceOutOfRange)
    );
    assert_eq!(
        sats_per_usd_at(MAX_USD_PER_BTC + 1),
        Err(Refusal::PriceOutOfRange)
    );
    assert_eq!(sats_per_usd_at(0), Err(Refusal::PriceOutOfRange));
    assert_eq!(sats_per_usd_at(u64::MAX), Err(Refusal::PriceOutOfRange));
}

#[test]
fn as_75_a_price_outside_the_range_is_refused_and_a_limit_left_alone_keeps_its_satoshis() {
    let rig = held(plain);
    for bad in [0, 999, 10_000_001] {
        let edit = SettingsEdit {
            usd_per_btc: Some(bad),
            node_per_sale: Some(30),
            ..SettingsEdit::default()
        };
        assert_eq!(
            rig.owner.settings_preview(&edit).unwrap_err(),
            Refusal::PriceOutOfRange
        );
    }
    // The same dollars at another price change nothing.
    let same = SettingsEdit {
        usd_per_btc: Some(50_000),
        light_per_sale: Some(10),
        light_budget: Some(60),
        ..SettingsEdit::default()
    };
    let preview = rig.owner.settings_preview(&same).unwrap();
    assert_eq!(preview.settings.light_per_sale, Limit::from_usd(10, 1_000));
    // Only the limit that changed is made at the price that was given.
    assert_eq!(preview.settings.light_budget, Limit::from_usd(60, 2_000));
    assert_eq!(preview.price, Some((50_000, 2_000)));
    let names: Vec<_> = preview.fields.iter().map(|f| f.field).collect();
    assert_eq!(names, ["light_budget"]);
    // A price is not used when no limit changed, and is not shown.
    let none = SettingsEdit {
        usd_per_btc: Some(50_000),
        tor_on: Some(true),
        ..SettingsEdit::default()
    };
    assert_eq!(rig.owner.settings_preview(&none).unwrap().price, None);
    // $98,500 gives 1,015 sats to a dollar: $100 is 101,500 satoshis.
    let odd = SettingsEdit {
        usd_per_btc: Some(98_500),
        node_per_sale: Some(100),
        ..SettingsEdit::default()
    };
    let preview = rig.owner.settings_preview(&odd).unwrap();
    assert_eq!(preview.settings.node_per_sale, Limit::from_usd(100, 1_015));
    assert_eq!(preview.settings.node_per_sale.sats, 101_500);
}

#[test]
fn as_76_a_node_is_named_by_its_key_or_its_address() {
    let key = node_key();
    assert_eq!(key[0], 2);
    // Capitals and small letters, and spaces round the text.
    assert_eq!(parse_lightning_node(&KEY.to_lowercase()), Ok(key));
    assert_eq!(parse_lightning_node(&format!("  {KEY}\n")), Ok(key));
    // An address gives its key and nothing else.
    for address in [
        format!("{KEY}@203.0.113.7:9735"),
        format!("{}@abcdefghijklmnop.onion:9735", KEY.to_lowercase()),
        format!("{KEY}@[2001:db8::1]:9735"),
        format!("{KEY}@node.example.com:1"),
    ] {
        assert_eq!(parse_lightning_node(&address), Ok(key), "{address}");
    }
}

#[test]
fn as_76_anything_else_is_refused_with_its_own_reason() {
    let fault = |text: &str| match parse_lightning_node(text) {
        Err(Refusal::LightningNode(f)) => f,
        other => panic!("{text}: {other:?}"),
    };
    // Not a key at all.
    assert_eq!(fault(""), NodeKeyFault::NotAKey);
    assert_eq!(fault("   "), NodeKeyFault::NotAKey);
    assert_eq!(fault("my node"), NodeKeyFault::NotAKey);
    assert_eq!(fault(&format!("{KEY}00")), NodeKeyFault::NotAKey);
    assert_eq!(fault(&format!("{KEY}zz")), NodeKeyFault::NotAKey);
    // An uncompressed key is longer than a node key.
    assert_eq!(fault(&"04".repeat(65)), NodeKeyFault::NotAKey);
    // A short form: cut short, with dots, or without the prefix byte.
    assert_eq!(fault(&KEY[..64]), NodeKeyFault::ShortForm);
    assert_eq!(fault(&KEY[..10]), NodeKeyFault::ShortForm);
    assert_eq!(
        fault(&format!("{}...{}", &KEY[..8], &KEY[58..])),
        NodeKeyFault::ShortForm
    );
    assert_eq!(
        fault(&format!("{}\u{2026}{}", &KEY[..8], &KEY[58..])),
        NodeKeyFault::ShortForm
    );
    assert_eq!(fault("02"), NodeKeyFault::ShortForm);
    // 66 digits that are not a compressed key.
    assert_eq!(
        fault(&format!("04{}", &KEY[2..])),
        NodeKeyFault::NotCompressed
    );
    assert_eq!(
        fault(&format!("00{}", &KEY[2..])),
        NodeKeyFault::NotCompressed
    );
    // A prefix of 02 with an x that is on no point of the curve.
    assert_eq!(
        fault(&format!("02{}", "00".repeat(32))),
        NodeKeyFault::NotOnCurve
    );
    assert_eq!(
        fault(&format!("02{}", "ff".repeat(32))),
        NodeKeyFault::NotOnCurve
    );
    // An address with no usable host and port.
    for bad in [
        format!("{KEY}@"),
        format!("{KEY}@host"),
        format!("{KEY}@host:"),
        format!("{KEY}@:9735"),
        format!("{KEY}@host:0"),
        format!("{KEY}@host:65536"),
        format!("{KEY}@host:port"),
        format!("{KEY}@ho st:9735"),
        format!("{KEY}@a@b:9735"),
    ] {
        assert_eq!(fault(&bad), NodeKeyFault::Address, "{bad}");
    }
    // The key is judged first.
    assert_eq!(fault("02ab@host"), NodeKeyFault::ShortForm);
}

#[test]
fn as_76_the_node_in_an_edit_is_named_removed_or_refused() {
    let rig = held(|s| s.lightning_node = Some(node_key()));
    let remove = SettingsEdit {
        lightning_node: Some(LightningEdit::Remove),
        ..SettingsEdit::default()
    };
    let preview = rig.owner.settings_preview(&remove).unwrap();
    assert_eq!(preview.settings.lightning_node, None);
    let bad = SettingsEdit {
        lightning_node: Some(LightningEdit::Set("02ab".to_string())),
        ..SettingsEdit::default()
    };
    assert_eq!(
        rig.owner.settings_preview(&bad).unwrap_err(),
        Refusal::LightningNode(NodeKeyFault::ShortForm)
    );
    // The same key again changes nothing.
    let same = SettingsEdit {
        lightning_node: Some(LightningEdit::Set(format!("{KEY}@host:9735"))),
        ..SettingsEdit::default()
    };
    assert!(rig.owner.settings_preview(&same).unwrap().fields.is_empty());
}

#[test]
fn as_77_a_preview_stores_and_changes_nothing() {
    let rig = held(plain);
    let before = rig.owner.snapshot().unwrap();
    let outbox = rig.owner.outbox().len();
    let stores = rig.owner_store.calls();
    for case in cases() {
        let mut edit = SettingsEdit {
            usd_per_btc: Some(PRICE),
            ..SettingsEdit::default()
        };
        (case.edit)(&mut edit);
        let _ = rig.owner.settings_preview(&edit);
    }
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    assert_eq!(rig.owner_store.calls(), stores);
    assert!(rig.owner.queued_settings().is_empty());
    assert_eq!(rig.owner.outbox().len(), outbox);
}

#[test]
fn as_77_the_preview_and_the_queue_agree_end_to_end() {
    let mut rig = held(plain);
    // A loosening edit with a tightening one in it is loosening as a whole.
    let edit = SettingsEdit {
        refund_cap: Some(150),
        usd_per_btc: Some(PRICE),
        tor_on: Some(true),
        ..SettingsEdit::default()
    };
    let preview = rig.owner.settings_preview(&edit).unwrap();
    assert_eq!(preview.change, Change::Loosening);
    assert_eq!(
        preview.fields,
        vec![
            FieldChange {
                field: "refund_cap",
                direction: Change::Loosening
            },
            FieldChange {
                field: "tor_on",
                direction: Change::Tightening
            },
        ]
    );
    let queued = ok(rig.owner.queue_settings(preview.settings.clone(), nop));
    assert!(matches!(queued, SettingsQueued::Queued(_)));
    // The Key app approves it and it is in force.
    rig.round(&[], 1);
    assert_eq!(rig.owner.known_settings(), Some(&preview.settings));
    // A tightening edit is signed by the owner alone and handed back.
    let tighten = SettingsEdit {
        refund_cap: Some(50),
        usd_per_btc: Some(PRICE),
        ..SettingsEdit::default()
    };
    let preview = rig.owner.settings_preview(&tighten).unwrap();
    assert_eq!(preview.change, Change::Tightening);
    assert_eq!(preview.settings.version, 3);
    let SettingsQueued::Tightening { signed, .. } =
        ok(rig.owner.queue_settings(preview.settings.clone(), nop))
    else {
        panic!("a tightening change is handed back");
    };
    assert_eq!(signed.settings, preview.settings);
    assert_eq!(rig.owner.known_settings().unwrap().version, 3);
}

#[test]
fn as_74_the_watch_key_in_an_edit_is_named_removed_and_sorted() {
    let key = openbmt_core::identity::Identity::from_secret_bytes(&[0x77; 32])
        .unwrap()
        .device_id();
    let rig = held(plain);
    let set = SettingsEdit {
        watch_key: Some(WatchEdit::Set(key)),
        ..SettingsEdit::default()
    };
    let preview = rig.owner.settings_preview(&set).unwrap();
    assert_eq!(preview.settings.watch_key, Some(key));
    assert_eq!(preview.change, Change::Loosening);
    assert_eq!(preview.fields.len(), 1);
    assert_eq!(preview.fields[0].field, "watch_key");
    let rig = held(|s| s.watch_key = Some(key));
    let remove = SettingsEdit {
        watch_key: Some(WatchEdit::Remove),
        ..SettingsEdit::default()
    };
    let preview = rig.owner.settings_preview(&remove).unwrap();
    assert_eq!(preview.settings.watch_key, None);
    assert_eq!(preview.change, Change::Tightening);
    // The same key again changes nothing.
    let same = SettingsEdit {
        watch_key: Some(WatchEdit::Set(key)),
        ..SettingsEdit::default()
    };
    assert!(rig.owner.settings_preview(&same).unwrap().fields.is_empty());
}

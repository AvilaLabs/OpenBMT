#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::approval::{classify, ChangeClass};
use openbmt_core::identity::Identity;
use openbmt_core::onion::onion_name;
use openbmt_core::settings::{classify_change, Change, OwnNode, Settings};

fn id(n: u8) -> openbmt_core::identity::DeviceId {
    Identity::from_secret_bytes(&[n % 200 + 1; 32])
        .unwrap()
        .device_id()
}

// Settings built from a script of two-byte steps: the first byte picks a
// field, the second its value.
fn build(steps: &[u8]) -> Settings {
    let mut s = Settings::defaults(1_000);
    for step in steps.chunks_exact(2) {
        let v = step[1];
        match step[0] % 19 {
            0 => s.light_per_sale.sats = u64::from(v),
            1 => s.node_budget.sats = u64::from(v),
            2 => s.underpayment_tolerance_bps = u32::from(v),
            3 => s.min_confirmations = v,
            4 => s.heartbeat_max_age_secs = u32::from(v),
            5 => s.tor_on = v & 1 == 1,
            6 => s.customer_wifi = v & 1 == 1,
            7 => s.min_sale_sats = u64::from(v),
            8 => {
                s.devices.insert(id(v % 8));
            }
            9 => {
                s.counters.insert(id(v % 8), u16::from(v) * 40);
            }
            10 => {
                let d = id(v % 8);
                s.devices.insert(d);
                s.boxes.insert(d);
            }
            11 => s.lightning_node = (v & 1 == 1).then(|| [v | 2; 33]),
            12 => s.watch_key = (v & 1 == 1).then(|| id(v)),
            13 => s.tips_on = v & 1 == 1,
            14 => s.refund_cap.display_usd = u32::from(v),
            15 => {
                s.own_node = (v & 1 == 1).then(|| OwnNode {
                    name: onion_name(&[v | 1; 32]),
                    port: 8333,
                })
            }
            16 => s.node_only = v & 1 == 1,
            17 => s.light_fallback = v & 1 == 1,
            _ => s.version = u32::from(v),
        }
    }
    s
}

// The class of any pair of settings is computed without panicking, no
// change is a Free one, a change that only tightens is Free, and a Redirect
// always names a Lightning node in the new settings.
fuzz_target!(|data: &[u8]| {
    let (a, b) = data.split_at(data.len() / 2);
    let (old, new) = (build(a), build(b));
    let class = classify(&old, &new);
    assert_eq!(classify(&old, &old), ChangeClass::Free);
    if classify_change(&old, &new) == Change::Tightening {
        assert_eq!(class, ChangeClass::Free);
    }
    if class == ChangeClass::Redirect {
        assert!(new.lightning_node.is_some());
        assert_ne!(old.lightning_node, new.lightning_node);
    }
});

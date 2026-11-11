//! The background watcher through `dispatch` (`docs/OWNER-APP-RULES.md`,
//! OA-80 to OA-89): the watch key made from its secret, the watch file read
//! strictly, the node's export of it, the settings change that names the
//! key, the refusal of every op of a node on the watcher's handle, and its
//! start, step and stop.

#![allow(clippy::result_large_err)]

use super::kit::*;
use super::screen::check_wire_words;
use super::wire::{Fault, Rd, W};
use crate::watch::{decode_file, public_key, WatchStart, MAX_WATCH_FILE, MAX_WATCH_PEERS};
use crate::{LinkConfig, OwnerNetwork, RouteView};

const OP_MAKE_KEY: u16 = 130;
const OP_NAMED: u16 = 131;
const OP_PREVIEW: u16 = 132;
const OP_EXPORT: u16 = 133;
const OP_START: u16 = 134;
const OP_STEP: u16 = 135;
const OP_STOP: u16 = 136;

fn fault(response: &[u8]) -> Fault {
    let f = Rd::error(response).expect("an error response");
    check_wire_words(&f.words).unwrap_or_else(|why| panic!("{why}: {f:?}"));
    f
}

fn secret(n: u8) -> Vec<u8> {
    vec![n; 32]
}

/// A file for a watcher whose folder is `dir`, aimed at nothing that answers.
fn sample(dir: &str) -> WatchStart {
    WatchStart {
        dir: dir.to_string(),
        network: OwnerNetwork::Regtest,
        secret: secret(7),
        proxy: None,
        fixed_peers: Vec::new(),
        route: RouteView::Direct {
            addr: "127.0.0.1:9".to_string(),
        },
        relay_key: id(0x41).device_id().to_bytes().to_vec(),
        counters: vec![counter_bytes()],
        heartbeat_every_secs: 60,
    }
}

/// A node with a link to one counter, and nothing in force yet.
fn linked() -> (Rig, u64) {
    let rig = Rig::new();
    let mut config = start_config(rig.dir().to_str().expect("UTF-8"));
    config.link = Some(LinkConfig {
        route: RouteView::Direct {
            addr: "127.0.0.1:9".to_string(),
        },
        relay_key: id(0x41).device_id().to_bytes().to_vec(),
        counters: vec![counter_bytes()],
        heartbeat_every_secs: 60,
        check_in_grace_secs: 3_600,
    });
    let handle = Rd::ok(&rig.call(1, 0, &W::new().start(&config).0)).u64();
    (rig, handle)
}

#[test]
fn the_watch_key_is_made_from_the_secret_and_nothing_else() {
    let rig = Rig::new();
    let response = rig.call(OP_MAKE_KEY, 0, &W::new().bytes(&secret(7)).0);
    let key = Rd::ok(&response).bytes();
    assert_eq!(key, public_key(&secret(7)).expect("a key"));
    assert_eq!(key.len(), 32);
    // Another secret, another key: the key is a function of the secret alone.
    let other = Rd::ok(&rig.call(OP_MAKE_KEY, 0, &W::new().bytes(&secret(8)).0)).bytes();
    assert_ne!(key, other);
    for bad in [
        vec![],
        vec![1u8; 31],
        vec![1u8; 33],
        vec![0u8; 32],
        vec![0xff; 32],
    ] {
        let f = fault(&rig.call(OP_MAKE_KEY, 0, &W::new().bytes(&bad).0));
        assert_eq!(f.rule, "watch-key");
    }
}

#[test]
fn the_watch_file_encodes_and_reads_back_exactly() {
    let start = sample("/data/node");
    let file = start.to_file();
    assert_eq!(&file[..4], b"OBM#");
    assert_eq!(file[4], 1);
    assert_eq!(decode_file(&file).expect("read"), start);
}

#[test]
fn the_watch_file_is_read_strictly() {
    let file = sample("/data/node").to_file();
    // Every cut is refused.
    for n in 0..file.len() {
        assert!(decode_file(&file[..n]).is_err(), "a cut at {n}");
    }
    // Anything left over is refused.
    let mut more = file.clone();
    more.push(0);
    assert!(decode_file(&more).is_err());
    // The magic and the version.
    let mut magic = file.clone();
    magic[0] ^= 1;
    assert!(decode_file(&magic).is_err());
    let mut version = file.clone();
    version[4] = 2;
    assert!(decode_file(&version).is_err());
    // Every flipped bit is refused, or read as a file that encodes to the same bytes.
    for i in 0..file.len() {
        for bit in 0..8 {
            let mut flipped = file.clone();
            flipped[i] ^= 1 << bit;
            if let Ok(s) = decode_file(&flipped) {
                assert_eq!(s.to_file(), flipped, "bit {bit} of byte {i}");
            }
        }
    }
    // The limits.
    let mut s = sample("/d");
    s.secret = vec![0; 32];
    assert!(
        decode_file(&s.to_file()).is_err(),
        "a secret that is not a key"
    );
    let mut s = sample("/d");
    s.secret = vec![7; 31];
    assert!(decode_file(&s.to_file()).is_err());
    let mut s = sample("/d");
    s.relay_key = vec![1; 31];
    assert!(decode_file(&s.to_file()).is_err());
    let mut s = sample("/d");
    s.counters = vec![vec![5; 32]; 65];
    assert!(decode_file(&s.to_file()).is_err(), "more than 64 counters");
    let mut s = sample("/d");
    s.counters = vec![vec![9; 31]];
    assert!(
        decode_file(&s.to_file()).is_err(),
        "a counter that is not 32 bytes"
    );
    let mut s = sample("/d");
    s.fixed_peers = vec!["127.0.0.1:1".to_string(); MAX_WATCH_PEERS + 1];
    assert!(decode_file(&s.to_file()).is_err());
    let mut s = sample("/d");
    s.dir = String::new();
    assert!(decode_file(&s.to_file()).is_err(), "no folder");
    let mut s = sample("/d");
    s.dir = "x".repeat(2_000);
    assert!(decode_file(&s.to_file()).is_err(), "a text over its limit");
    assert!(decode_file(&vec![0; MAX_WATCH_FILE + 1]).is_err());
    // A cut file is the reader's own refusal; a bad magic or limit is the watch file's.
    assert_eq!(decode_file(b"OBM1\x01").unwrap_err().rule(), "watch-file");
}

#[test]
fn the_export_holds_the_link_and_the_secret_and_no_wallet() {
    let (rig, handle) = linked();
    let file = Rd::ok(&rig.call(OP_EXPORT, handle, &W::new().bytes(&secret(7)).0)).bytes();
    let s = decode_file(&file).expect("the file the watcher starts from");
    assert_eq!(s.secret, secret(7));
    assert_eq!(s.counters, vec![counter_bytes()]);
    assert_eq!(s.relay_key, id(0x41).device_id().to_bytes().to_vec());
    assert_eq!(s.network, OwnerNetwork::Regtest);
    assert_eq!(s.dir, rig.dir().to_string_lossy());
    // Nothing of the wallet or of the owner's key is in the bytes.
    let k = kit();
    let wallet = k.wallet.canonical_string().to_string();
    let text = String::from_utf8_lossy(&file).to_string();
    assert!(!text.contains(&wallet));
    assert!(!file.windows(32).any(|w| w == OWNER_SECRET.as_slice()));
    // A secret that is not a key is refused.
    let f = fault(&rig.call(OP_EXPORT, handle, &W::new().bytes(&[0; 32]).0));
    assert_eq!(f.rule, "watch-file");
}

#[test]
fn a_node_with_no_link_has_nothing_to_export() {
    let rig = Rig::new();
    let handle = rig.start();
    let f = fault(&rig.call(OP_EXPORT, handle, &W::new().bytes(&secret(7)).0));
    assert_eq!(f.rule, "watch-no-link");
}

#[test]
fn naming_the_watch_key_loosens_and_removing_it_tightens() {
    let (rig, handle) = super::settings::held();
    let named = Rd::ok(&rig.call(OP_NAMED, handle, &[])).opt(Rd::bytes);
    assert_eq!(named, None);
    let key = id(0x33).device_id().to_bytes().to_vec();
    let response = rig.call(OP_PREVIEW, handle, &W::new().u8(1).bytes(&key).0);
    let mut r = Rd::ok(&response);
    let _version = r.u32();
    let changes = r.list(|r| {
        let field = r.string();
        let _label = r.string();
        let _counter = r.opt(Rd::bytes);
        let _old = r.string();
        let new = r.string();
        let loosening = r.bool();
        (field, new, loosening)
    });
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].0, "watch_key");
    assert!(changes[0].2, "naming a watch key loosens");
    assert!(r.bool(), "the Key app must co-sign");
    assert!(!r.bool(), "something changed");
    // Removing a key that is not named changes nothing.
    let response = rig.call(OP_PREVIEW, handle, &W::new().u8(0).bytes(&[]).0);
    let mut r = Rd::ok(&response);
    let _ = r.u32();
    let none = r.list(|r| {
        let _ = (
            r.string(),
            r.string(),
            r.opt(Rd::bytes),
            r.string(),
            r.string(),
            r.bool(),
        );
    });
    assert!(none.is_empty());
    assert!(!r.bool());
    assert!(r.bool(), "nothing changed");
    // A key that is not 32 bytes is refused with words.
    let f = fault(&rig.call(OP_PREVIEW, handle, &W::new().u8(1).bytes(&[1, 2, 3]).0));
    assert!(!f.rule.is_empty());
}

#[test]
fn the_watcher_starts_steps_and_stops_and_refuses_what_a_node_does() {
    let scratch = Scratch::new();
    let rig = Rig::new();
    let file = sample(scratch.path().to_str().expect("UTF-8")).to_file();
    let handle = Rd::ok(&rig.call(OP_START, 0, &W::new().bytes(&file).0)).u64();
    // One turn: nothing answers at the relay, so nothing was taken and nobody said refused.
    let response = rig.call(OP_STEP, handle, &W::new().u64(T0).u32(0).0);
    let mut r = Rd::ok(&response);
    let sent = r.u64();
    let _reached = r.bool();
    let refused = r.bool();
    let _notes = r.list(Rd::string);
    r.done();
    assert_eq!(sent, 0);
    assert!(!refused);
    // Every op of a node refuses this handle, with words.
    for op in [10u16, 11, 20, 30, 40, 46, 47, 131, 133] {
        let f = fault(&rig.call(op, handle, &[]));
        assert_eq!(f.rule, "watch-only", "op {op}");
        assert!(f.words.next.contains("owner app"), "{}", f.words.next);
    }
    Rd::ok(&rig.call(OP_STOP, handle, &[]));
    let f = fault(&rig.call(OP_STEP, handle, &W::new().u64(T0).u32(0).0));
    assert!(
        f.rule == "handle-unknown" || f.rule == "watch-stopped",
        "{}",
        f.rule
    );
}

#[test]
fn a_file_that_is_not_a_watch_file_does_not_start_a_watcher() {
    let rig = Rig::new();
    let f = fault(&rig.call(OP_START, 0, &W::new().bytes(b"not a file").0));
    assert_eq!(f.rule, "watch-file");
}

#[test]
fn the_kotlin_binding_has_the_watcher() {
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bindings/kotlin/org/openbmt/owner/OwnerNode.kt"),
    )
    .expect("the binding");
    for needle in [
        "OP_WATCH_MAKE_KEY = 130",
        "OP_WATCH_STOP = 136",
        "class OwnerWatch private constructor",
        "fun watchExport(secret: ByteArray)",
        "WatchStep(r.u64(), r.bool(), r.bool(), r.strings())",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
}

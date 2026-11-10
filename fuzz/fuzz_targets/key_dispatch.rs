#![no_main]

use std::cell::RefCell;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use bitcoin::bip32::{DerivationPath, Fingerprint, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pairing::CounterPairing;
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_key_ffi::{dispatch, KeyApp, KeyStore, Registry, StoreError};
use openbmt_owner::{Options, Owner};

const NOW: u64 = 10_000_000;
const OWNER_SECRET: [u8; 32] = [0x11; 32];
/// The world is made again after this many inputs, so that what one input
/// changed (a full allowlist, a closed app) does not hide the next.
const REMAKE_EVERY: u32 = 256;

/// An in-memory store: it keeps the last bytes saved and draws bytes from a
/// counter.
struct Mem {
    saved: Mutex<Option<Vec<u8>>>,
    drawn: Mutex<u64>,
}

impl KeyStore for Mem {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.saved.lock().unwrap().clone())
    }

    fn save(&self, bytes: Vec<u8>) -> Result<(), StoreError> {
        *self.saved.lock().unwrap() = Some(bytes);
        Ok(())
    }

    fn random(&self, n: u32) -> Result<Vec<u8>, StoreError> {
        let mut drawn = self.drawn.lock().unwrap();
        let mut out = Vec::new();
        while out.len() < n as usize {
            *drawn += 1;
            let mut x = drawn.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x1234_5678_9abc_def1;
            for _ in 0..4 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        out.truncate(n as usize);
        Ok(out)
    }

    fn now(&self) -> u64 {
        NOW
    }
}

fn frame(op: u16, handle: u64, fields: &[u8]) -> Vec<u8> {
    let mut out = b"OBM8".to_vec();
    out.push(1);
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&handle.to_le_bytes());
    out.extend_from_slice(fields);
    out
}

fn bytes_field(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}

/// A reader of the result of an op.
struct Rd<'a>(&'a [u8]);

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        a
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().unwrap())
    }

    fn bytes(&mut self) -> Vec<u8> {
        let n = self.u32() as usize;
        self.take(n).to_vec()
    }

    fn string(&mut self) -> String {
        String::from_utf8(self.bytes()).unwrap()
    }
}

/// The result of a call that must have worked.
fn ok(reg: &Registry, store: &Arc<Mem>, op: u16, handle: u64, fields: &[u8]) -> Vec<u8> {
    let response = dispatch(reg, store.clone(), &frame(op, handle, fields));
    assert_eq!(&response[..4], b"OBM9");
    assert_eq!(response[5], 0, "setup op {op} failed");
    response[6..].to_vec()
}

fn account(seed: u8, index: u8) -> KeySlot {
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(Network::Regtest, &[seed; 32]).unwrap();
    let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
    let key = master.derive_priv(&secp, &path).unwrap();
    KeySlot {
        index,
        origin_fingerprint: master.fingerprint(&secp),
        origin_path: path,
        xpub: Xpub::from_priv(&secp, &key),
    }
}

/// A registry with a Key app that has its wallet (handle 1) and one open
/// session (handle 2), on a store of its own.
struct World {
    reg: Registry,
    store: Arc<Mem>,
    runs: u32,
    /// The stored bytes that were last checked to restore.
    checked: Option<Vec<u8>>,
}

fn world() -> World {
    let store = Arc::new(Mem {
        saved: Mutex::new(None),
        drawn: Mutex::new(0),
    });
    let reg = Registry::new();
    let app = u64::from_le_bytes(ok(&reg, &store, 1, 0, &[])[..8].try_into().unwrap());
    assert_eq!(app, 1);
    // Platform entropy, regtest.
    ok(&reg, &store, 11, app, &[0, 3]);
    // Key 2's public parts, to build the wallet from.
    let public = ok(&reg, &store, 14, app, &[]);
    let mut r = Rd(&public);
    assert_eq!(r.take(2), [1, 3]);
    let fingerprint = r.string();
    let _origin = r.string();
    let xpub = r.string();
    let slot2 = KeySlot {
        index: 2,
        origin_fingerprint: Fingerprint::from_str(&fingerprint).unwrap(),
        origin_path: "m/48'/1'/0'/2'".parse().unwrap(),
        xpub: xpub.parse().unwrap(),
    };
    let slot3 = account(3, 3);
    let key3 = slot3.xpub.to_string();
    let wallet =
        WalletDescription::new(Network::Regtest, [account(1, 1), slot2, slot3]).unwrap();
    let owner_root = Identity::from_secret_bytes(&OWNER_SECRET)
        .unwrap()
        .device_id()
        .to_bytes();
    let mut setup = Vec::new();
    bytes_field(&mut setup, wallet.canonical_string().as_bytes());
    setup.push(3);
    bytes_field(&mut setup, &owner_root);
    setup.push(1);
    bytes_field(&mut setup, key3.as_bytes());
    setup.push(1);
    setup.extend_from_slice(&3u32.to_le_bytes());
    ok(&reg, &store, 17, app, &setup);

    // A request with a pairing and the health check, from the owner side.
    let root = Rd(&ok(&reg, &store, 15, app, &[])).bytes();
    let approver = DeviceId::from_bytes(&root.try_into().unwrap()).unwrap();
    let mut owner = Owner::new(
        &OWNER_SECRET,
        wallet,
        approver,
        BatchPolicy { min_count: 3 },
        Options::default(),
    )
    .unwrap();
    let offer = Rd(&ok(&reg, &store, 19, app, &[])).bytes();
    let counter = Identity::from_secret_bytes(&[0x31; 32]).unwrap();
    let (_, pairing) = CounterPairing::begin(&offer, &counter, "Counter 1", [1; 32], NOW).unwrap();
    owner
        .queue_pairing(&pairing.to_bytes(), |_| Ok::<(), String>(()))
        .unwrap();
    let request = owner
        .build_request(&[], 1_000, 0, 0, NOW, &[7; 96], |_| Ok::<(), String>(()))
        .unwrap()
        .request;
    let mut fields = Vec::new();
    bytes_field(&mut fields, &request);
    ok(&reg, &store, 30, app, &fields);
    World {
        reg,
        store,
        runs: 0,
        checked: None,
    }
}

thread_local! {
    static WORLD: RefCell<Option<World>> = const { RefCell::new(None) };
}

fuzz_target!(|data: &[u8]| {
    WORLD.with(|cell| {
        let mut slot = cell.borrow_mut();
        let w = match slot.as_mut() {
            Some(w) if w.runs < REMAKE_EVERY => w,
            _ => slot.insert(world()),
        };
        w.runs += 1;
        // The first byte picks how the rest is read: whole, or as the fields
        // of a frame addressed to the app, the session or no object.
        let request = match data.split_first() {
            Some((mode, rest)) if mode % 4 != 0 && rest.len() >= 2 => {
                let op = u16::from_le_bytes([rest[0], rest[1]]);
                let handle = match mode % 4 {
                    1 => 1,
                    2 => 2,
                    _ => 0,
                };
                frame(op % 80, handle, &rest[2..])
            }
            _ => data.to_vec(),
        };
        let response = dispatch(&w.reg, w.store.clone(), &request);
        // Always a frame, and never an internal error.
        assert!(response.len() >= 6);
        assert_eq!(&response[..4], b"OBM9");
        assert_eq!(response[4], 1);
        assert!(response[5] <= 3, "status {}: {:?}", response[5], &response[6..]);
        // The stored bytes always restore a Key app.
        let saved = w.store.saved.lock().unwrap().clone();
        if saved != w.checked {
            let again = Arc::new(Mem {
                saved: Mutex::new(saved.clone()),
                drawn: Mutex::new(0),
            });
            KeyApp::open(again).expect("the stored bytes restore");
            w.checked = saved;
        }
        // A closed app ends this world.
        let probe = dispatch(&w.reg, w.store.clone(), &frame(10, 1, &[]));
        if probe[5] != 0 {
            w.runs = REMAKE_EVERY;
        }
    });
});

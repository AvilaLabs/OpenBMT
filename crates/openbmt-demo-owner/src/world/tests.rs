//! The node of the demo owner, started on the demo shop's wallet and driven through
//! the framed calls, on the pretend chain: a payment to the wallet's address
//! is seen and mined, and the node watches the wallet's first address.

use super::*;
use crate::{call, set_now, shop};

const T0: u64 = 1_700_000_000;
/// Under the system's temporary folder: a real folder when the workspace is
/// built with the network side, and only a name in the build whose files are in
/// memory.
fn dir() -> String {
    let dir = std::env::temp_dir().join(format!("openbmt-demo-owner-world-{}", std::process::id()));
    dir.to_str().expect("UTF-8").to_string()
}

fn frame(op: u16, handle: u64, fields: &[u8]) -> Vec<u8> {
    let mut out = b"OBMk".to_vec();
    out.push(1);
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&handle.to_le_bytes());
    out.extend_from_slice(fields);
    out
}

fn put_bytes(out: &mut Vec<u8>, v: &[u8]) {
    out.extend_from_slice(&(v.len() as u32).to_le_bytes());
    out.extend_from_slice(v);
}

/// The fields of `start` for the demo shop on regtest, with the Key app's three keys.
fn start_fields() -> Vec<u8> {
    let mut f = Vec::new();
    put_bytes(&mut f, dir().as_bytes());
    f.push(3); // regtest
    f.extend_from_slice(&T0.to_le_bytes());
    put_bytes(&mut f, &shop::OWNER_SECRET);
    put_bytes(&mut f, &shop::approver_root().expect("a root"));
    put_bytes(&mut f, shop::wallet_text().expect("a wallet").as_bytes());
    put_bytes(&mut f, &shop::key1().expect("a key"));
    f.push(0); // no Key 3
    f.extend_from_slice(&1_000u32.to_le_bytes());
    f.extend_from_slice(&6u32.to_le_bytes());
    f.extend_from_slice(&0u32.to_le_bytes()); // birthday
    f.push(0); // no proxy
    f.extend_from_slice(&0u32.to_le_bytes()); // no first peers
    f.push(0); // no sending proxy
    f.push(1); // without Tor
    f.push(0); // no link
    f.push(0); // not an own wallet
    f.push(0); // no float
    f
}

fn ok(response: &[u8]) -> &[u8] {
    assert_eq!(response.get(..4), Some(b"OBMl".as_slice()));
    assert_eq!(response.get(5), Some(&0), "the call failed: {response:?}");
    response.get(6..).expect("a body")
}

#[test]
fn a_payment_to_the_shop_wallet_is_seen_and_mined_and_its_first_address_is_watched() {
    // Built together with the Android library, the owner's code has its
    // network side and takes no regtest wallet: the demo runs only without it.
    if !openbmt_owner_ffi::DEMO_CHAIN {
        return;
    }
    set_now(T0);
    let started = call(&frame(1, 0, &start_fields()));
    let handle = u64::from_le_bytes(ok(&started).get(..8).expect("a handle").try_into().unwrap());

    // The first events: the chain is healthy.
    let mut pump = Vec::new();
    pump.extend_from_slice(&T0.to_le_bytes());
    pump.extend_from_slice(&0u32.to_le_bytes());
    ok(&call(&frame(10, handle, &pump)));

    // The node watches the wallet's receive scripts from the first address (and a gap of them), which is where the
    // counter's first sale is paid to.
    let first = shop::address(0).expect("an address");
    {
        let guard = world();
        let w = guard.as_ref().expect("a chain");
        let has = |address: &str| {
            let script = Address::from_str(address)
                .expect("reads")
                .require_network(NETWORK)
                .expect("regtest")
                .script_pubkey();
            w.watched.contains(&script)
        };
        assert!(has(&first), "the first address is watched");
    }

    // A sale of 8.00 USD at 100,000 USD per bitcoin: 8,000 sats.
    pay(&first, 8_000).expect("paid");
    ok(&call(&frame(10, handle, &pump)));
    set_now(T0 + 600);
    assert_eq!(mine().expect("mined"), 1);
    ok(&call(&frame(10, handle, &pump)));

    ok(&call(&frame(2, handle, &[])));
}

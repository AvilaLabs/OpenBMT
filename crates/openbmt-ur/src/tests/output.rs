//! UR-32: the `crypto-output` of a shared wallet.

use super::key::{
    MAIN_CHAIN, MAIN_FP, MAIN_KEY, MAIN_PARENT, OTHER_CHAIN, OTHER_FP, OTHER_KEY, OTHER_PARENT,
    TEST_CHAIN, TEST_KEY, TEST_PARENT,
};
use crate::cbor_read::{read, Item};
use crate::output::hdkey;
use crate::{
    output_to_ur_parts, ur_to_key_text, wsh_sortedmulti_cbor, Decoder, OutputKey, UrError,
    TYPE_CRYPTO_OUTPUT,
};

fn unhex<const N: usize>(s: &str) -> [u8; N] {
    let v: Vec<u8> = (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex"))
        .collect();
    v.try_into().expect("length")
}

fn origin(coin: u32) -> Vec<(u32, bool)> {
    vec![(48, true), (coin, true), (0, true), (2, true)]
}

fn key(fp: u32, parent: u32, k: &str, c: &str, coin: u32, chain: u32) -> OutputKey {
    OutputKey {
        master_fingerprint: fp.to_be_bytes(),
        origin: origin(coin),
        parent_fingerprint: parent.to_be_bytes(),
        key: unhex(k),
        chain_code: unhex(c),
        testnet: coin == 1,
        chain,
    }
}

fn three(chain: u32) -> Vec<OutputKey> {
    vec![
        key(MAIN_FP, MAIN_PARENT, MAIN_KEY, MAIN_CHAIN, 0, chain),
        key(OTHER_FP, OTHER_PARENT, OTHER_KEY, OTHER_CHAIN, 0, chain),
        key(MAIN_FP, TEST_PARENT, TEST_KEY, TEST_CHAIN, 0, chain),
    ]
}

fn tagged<'a>(item: &'a Item<'a>, tag: u64) -> &'a Item<'a> {
    match item {
        Item::Tag(t, inner) if *t == tag => inner,
        other => panic!("not tag {tag}: {other:?}"),
    }
}

#[test]
fn ur_32_the_cbor_is_the_script_expression_around_the_keys() {
    let cbor = wsh_sortedmulti_cbor(2, &three(0)).expect("cbor");
    // The head: tag 401, tag 407, a map of two.
    assert_eq!(
        cbor.get(..6),
        Some(&[0xd9, 0x01, 0x91, 0xd9, 0x01, 0x97][..])
    );
    let item = read(&cbor).expect("strict");
    let sorted = tagged(tagged(&item, 401), 407);
    assert_eq!(sorted.get(1).and_then(Item::uint), Some(2));
    let Some(Item::Array(keys)) = sorted.get(2) else {
        panic!("keys");
    };
    assert_eq!(keys.len(), 3);
    for (k, want) in keys.iter().zip(three(0)) {
        let map = tagged(k, 303);
        let Item::Map(entries) = map else { panic!() };
        let numbers: Vec<u64> = entries.iter().map(|(n, _)| *n).collect();
        assert_eq!(numbers, [3, 4, 5, 6, 7, 8]);
        assert_eq!(map.get(3), Some(&Item::Bytes(&want.key)));
        assert_eq!(map.get(4), Some(&Item::Bytes(&want.chain_code)));
        // The coin: bitcoin, main network.
        let info = tagged(map.get(5).expect("coin"), 305);
        assert_eq!(info.get(1).and_then(Item::uint), Some(0));
        assert_eq!(info.get(2).and_then(Item::uint), Some(0));
        // The origin.
        let origin = tagged(map.get(6).expect("origin"), 304);
        let Some(Item::Array(steps)) = origin.get(1) else {
            panic!("steps")
        };
        assert_eq!(
            steps,
            &[
                Item::Uint(48),
                Item::Bool(true),
                Item::Uint(0),
                Item::Bool(true),
                Item::Uint(0),
                Item::Bool(true),
                Item::Uint(2),
                Item::Bool(true)
            ]
        );
        assert_eq!(
            origin.get(2).and_then(Item::uint),
            Some(u64::from(u32::from_be_bytes(want.master_fingerprint)))
        );
        assert_eq!(origin.get(3).and_then(Item::uint), Some(4));
        // The children: step 0, then the wildcard.
        let children = tagged(map.get(7).expect("children"), 304);
        assert_eq!(
            children.get(1),
            Some(&Item::Array(vec![
                Item::Uint(0),
                Item::Bool(false),
                Item::Array(vec![]),
                Item::Bool(false)
            ]))
        );
        assert_eq!(
            map.get(8).and_then(Item::uint),
            Some(u64::from(u32::from_be_bytes(want.parent_fingerprint)))
        );
    }
    // The change form differs in the step only.
    let change = wsh_sortedmulti_cbor(2, &three(1)).expect("cbor");
    assert_eq!(change.len(), cbor.len());
    assert_ne!(change, cbor);
}

#[test]
fn ur_32_each_key_reads_back_as_the_text_the_ceremony_takes() {
    let mut v = Vec::new();
    hdkey(&mut v, &three(0)[0]);
    let text = ur_to_key_text("crypto-hdkey", &v).expect("a key");
    assert_eq!(
        text.as_str(),
        format!("[56c4fac3/48h/0h/0h/2h]{}", super::key::MAIN_XPUB)
    );
    let mut v = Vec::new();
    hdkey(&mut v, &three(1)[1]);
    let text = ur_to_key_text("crypto-hdkey", &v).expect("a key");
    assert_eq!(
        text.as_str(),
        format!("[e2867bb6/48h/0h/0h/2h]{}", super::key::OTHER_XPUB)
    );
    // A test-network key carries the network and the coin 1.
    let k = key(MAIN_FP, TEST_PARENT, TEST_KEY, TEST_CHAIN, 1, 0);
    let mut v = Vec::new();
    hdkey(&mut v, &k);
    let text = ur_to_key_text("crypto-hdkey", &v).expect("a key");
    assert_eq!(
        text.as_str(),
        format!("[56c4fac3/48h/1h/0h/2h]{}", super::key::TEST_TPUB)
    );
}

#[test]
fn ur_32_the_parts_read_back_as_a_crypto_output() {
    let cbor = wsh_sortedmulti_cbor(2, &three(0)).expect("cbor");
    let mut e = output_to_ur_parts(&cbor, 100).expect("parts");
    assert!(!e.is_single_part());
    let mut d = Decoder::new();
    for _ in 0..(e.seq_len() * 3) {
        if d.receive(&e.next_part_upper()).expect("good").complete {
            break;
        }
    }
    assert_eq!(d.result(), Some((TYPE_CRYPTO_OUTPUT.to_string(), cbor)));
}

#[test]
fn ur_32_a_threshold_or_a_count_out_of_range_is_refused() {
    let keys = three(0);
    for t in [0u8, 4] {
        assert!(matches!(
            wsh_sortedmulti_cbor(t, &keys),
            Err(UrError::Output(_))
        ));
    }
    assert!(matches!(
        wsh_sortedmulti_cbor(1, &[]),
        Err(UrError::Output(_))
    ));
    let many: Vec<OutputKey> = (0..16).map(|_| keys[0].clone()).collect();
    assert!(matches!(
        wsh_sortedmulti_cbor(2, &many),
        Err(UrError::Output(_))
    ));
    let mut bare = keys.clone();
    bare[1].origin.clear();
    let e = wsh_sortedmulti_cbor(2, &bare).unwrap_err();
    assert_eq!(e.rule(), "UR-32");
    assert!(e.to_string().starts_with("UR-32"));
    let mut deep = keys;
    deep[0].origin = vec![(1, false); 17];
    assert!(wsh_sortedmulti_cbor(2, &deep).is_err());
}

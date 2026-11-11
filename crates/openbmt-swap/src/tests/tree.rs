//! The key of BIP 327 and the output of the tree (SW-25, SW-26).

use bitcoin::hex::FromHex;
use bitcoin::secp256k1::PublicKey;

use super::fixtures::*;
use crate::tree::{KeyOrder, SwapTree};
use crate::{key_agg, Refused};

const KEYS: [&str; 3] = [
    "02F9308A019258C31049344F85F89D5229B531C845836F99B08601F113BCE036F9",
    "03DFF1D77F2A671C5F36183726DB2341BE58FEAE1DA2DECED843240F7B502BA659",
    "023590A94E768F8E1815C2F24B4D80A8E3149316C3518CE7B7AD338368D038CA66",
];

fn key(i: usize) -> PublicKey {
    PublicKey::from_slice(&Vec::<u8>::from_hex(KEYS[i]).unwrap()).unwrap()
}

fn agg(indices: &[usize]) -> String {
    let keys: Vec<PublicKey> = indices.iter().map(|i| key(*i)).collect();
    key_agg(&keys)
        .unwrap()
        .serialize()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect()
}

#[test]
fn sw_26_the_key_aggregation_matches_the_vectors_of_bip_327() {
    // The valid cases of `key_agg_vectors.json` of BIP 327.
    assert_eq!(
        agg(&[0, 1, 2]),
        "90539EEDE565F5D054F32CC0C220126889ED1E5D193BAF15AEF344FE59D4610C"
    );
    assert_eq!(
        agg(&[2, 1, 0]),
        "6204DE8B083426DC6EAF9502D27024D53FC826BF7D2012148A0575435DF54B2B"
    );
    assert_eq!(
        agg(&[0, 0, 0]),
        "B436E3BAD62B8CD409969A224731C193D051162D8C5AE8B109306127DA3AA935"
    );
    assert_eq!(
        agg(&[0, 0, 1, 1]),
        "69BC22BFA5D106306E48A20679DE1D7389386124D07571D0D872686028C26A3E"
    );
}

#[test]
fn sw_26_no_keys_are_refused() {
    assert!(key_agg(&[]).is_err());
    assert_eq!(key_agg(&[]).unwrap_err(), Refused::BadRefundKey);
}

#[test]
fn sw_26_the_order_of_the_keys_changes_the_output() {
    let s = secrets();
    let a = SwapTree::build(
        &s.claim_public(),
        &service_key(),
        &s.preimage_hash(),
        TIMEOUT,
        KeyOrder::RefundFirst,
    )
    .unwrap();
    // The aggregate of the other order is another key.
    let swapped = key_agg(&[s.claim_public(), service_key()]).unwrap();
    assert_ne!(a.internal_key(), swapped);
    // The output is a version 1 witness program of 32 bytes.
    assert!(a.script_pubkey().is_p2tr());
    assert!(a.address(NET).to_string().starts_with("tb1p"));
    assert!(a
        .address(bitcoin::Network::Bitcoin)
        .to_string()
        .starts_with("bc1p"));
    assert_eq!(
        KeyOrder::from_byte(KeyOrder::RefundFirst.to_byte()),
        Some(KeyOrder::RefundFirst)
    );
    assert_eq!(KeyOrder::from_byte(1), None);
}

#[test]
fn sw_25_the_claim_leaf_is_in_the_tree_and_spends_with_a_control_block() {
    let swap = good_swap();
    let control = swap.tree.claim_control_block().unwrap();
    let spk = swap.tree.script_pubkey();
    let output_key = bitcoin::XOnlyPublicKey::from_slice(&spk.as_bytes()[2..]).unwrap();
    assert!(control.verify_taproot_commitment(
        &bitcoin::secp256k1::Secp256k1::new(),
        output_key,
        swap.tree.claim_leaf()
    ));
    // Not the refund leaf's commitment, and not another key's.
    assert!(!control.verify_taproot_commitment(
        &bitcoin::secp256k1::Secp256k1::new(),
        output_key,
        swap.tree.refund_leaf()
    ));
}

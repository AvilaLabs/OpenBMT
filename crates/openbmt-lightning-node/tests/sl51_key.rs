//! SL-51: the key. The Shop Lightning child the stick makes equals the one
//! the node's own code makes, for the same root and index, byte for byte, and
//! the node ID the stick shows equals the core's. The signer makes no BIP-85
//! child (it has no BIP-85 code), so there is nothing of it to compare.
//!
//! The published BIP-85 vectors are in `src/bip85.rs`
//! (`sl_10_spec_hex_vector`, `sl_10_spec_base_vectors`) and in the stick's
//! `tests/sk_sign.rs` (`sk_34_the_children_equal_the_bip85_specification_vectors`);
//! this file adds the vector once more through both crates side by side.

use std::str::FromStr;

use bitcoin::bip32::Xpriv;
use openbmt_core::shop_lightning::node_id;
use openbmt_lightning_node::bip85::{node_seed, NODE_SEED_INDEX};
use openbmt_stick::bip85 as stick;
use openbmt_stick::outputs::master_of;

/// The root of every vector of the BIP-85 text.
const SPEC_XPRV: &str = "xprv9s21ZrQH143K2LBWUUQRFXhucrQqBpKdRRxNVq2zBqsx8HVqFk2uYo8kmbaLLHRdqtQpUm98uKfu3vca1LqdGhUtyoFnCNkfmXRyPXLjbKb";
const SPEC_HEX_64: &str = "492db4698cf3b73a5a24998aa3e9d7fa96275d85724a91e71aa2d645442f878555d078fd1f1f67e368976f04137b1f7a0d19232136ca50c44614af72b5582a5c";

const WORDS_A: [&str; 12] = [
    "abandon", "abandon", "abandon", "abandon", "abandon", "abandon", "abandon", "abandon",
    "abandon", "abandon", "abandon", "about",
];
const WORDS_B: [&str; 12] = [
    "legal", "winner", "thank", "year", "wave", "sausage", "worth", "useful", "legal", "winner",
    "thank", "yellow",
];

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn roots() -> Vec<(&'static str, Xpriv)> {
    vec![
        ("spec", Xpriv::from_str(SPEC_XPRV).unwrap()),
        ("abandon", master_of(&WORDS_A).unwrap()),
        ("legal", master_of(&WORDS_B).unwrap()),
    ]
}

#[test]
fn sl_51_the_vector_is_the_same_through_the_stick_and_the_node() {
    let master = Xpriv::from_str(SPEC_XPRV).unwrap();
    let node = node_seed(&master, NODE_SEED_INDEX).unwrap();
    let from_stick = stick::entropy_at(&master, &stick::lightning_path()).unwrap();
    assert_eq!(hex(&node.0), SPEC_HEX_64);
    assert_eq!(hex(&from_stick), SPEC_HEX_64);
}

#[test]
fn sl_51_the_stick_child_equals_the_nodes_for_three_roots_and_several_indexes() {
    for (name, master) in roots() {
        for index in [0u32, 1, 2, 7, 99, 2_147_483_647] {
            let node = node_seed(&master, index).unwrap();
            let path = format!("m/83696968'/128169'/64'/{index}'");
            let from_stick = stick::entropy_at(&master, &path).unwrap();
            assert_eq!(node.0, from_stick, "root {name}, index {index}");
        }
    }
}

#[test]
fn sl_51_the_stick_child_of_the_words_equals_the_nodes_at_the_rules_index() {
    for words in [&WORDS_A, &WORDS_B] {
        let master = master_of(words).unwrap();
        let node = node_seed(&master, NODE_SEED_INDEX).unwrap();
        let from_stick = stick::lightning_child(words).unwrap();
        assert_eq!(node.0, from_stick);
        assert_eq!(stick::lightning_hex(words).unwrap(), hex(&node.0));
    }
}

#[test]
fn sl_51_the_stick_node_id_equals_the_cores_for_the_nodes_seed() {
    for words in [&WORDS_A, &WORDS_B] {
        let master = master_of(words).unwrap();
        let node = node_seed(&master, NODE_SEED_INDEX).unwrap();
        assert_eq!(
            stick::lightning_node_id_hex(words).unwrap(),
            hex(&node_id(&node.0))
        );
    }
}

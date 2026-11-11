//! The description of the protocol: generated from the encoder's own tables
//! into `bindings/PROTOCOL.md`, and compared with the hand-written Kotlin.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::call::{enum_tags, ops, type_names};
use crate::protocol_description;

fn bindings() -> PathBuf {
    std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("bindings")
}

#[test]
fn op_numbers_and_names_are_unique() {
    let table = ops();
    let numbers: BTreeSet<u16> = table.iter().map(|o| o.num).collect();
    let names: BTreeSet<&str> = table.iter().map(|o| o.name).collect();
    assert_eq!(numbers.len(), table.len());
    assert_eq!(names.len(), table.len());
}

#[test]
fn the_op_numbers_are_the_ones_the_readme_lists() {
    // Numbers are never reused: this list only ever grows.
    let pinned: &[(u16, &str)] = &[
        (1, "open"),
        (2, "close_app"),
        (3, "close_session"),
        (10, "state"),
        (11, "create_from_entropy"),
        (12, "create_approver_only"),
        (13, "key_words"),
        (14, "key2_public"),
        (15, "device_root"),
        (16, "device_root_words"),
        (17, "accept_wallet"),
        (18, "wallet_view"),
        (19, "pair_offer_bytes"),
        (20, "check_kit"),
        (30, "open_session"),
        (31, "session_id"),
        (32, "lines"),
        (33, "decide"),
        (34, "finish"),
        (35, "cancel"),
        (40, "signer_status"),
        (41, "receive_mark"),
        (42, "enroll_destination"),
        (43, "remove_destination"),
        (44, "set_ceilings"),
        (45, "set_fee_ceilings"),
        (46, "raise_change_mark"),
        (47, "raise_receive_mark"),
        (48, "correct_clock"),
        (50, "make_invitation"),
        (51, "open_invitation"),
        (52, "cancel_invitation"),
        (60, "coin_bits"),
        (61, "dice_bits"),
        (62, "dice_face_counts"),
        (63, "word_suggestions"),
        (64, "word_is_valid"),
        (65, "parse_deposit_code"),
        (80, "take_key3"),
        (81, "key3_view"),
        (82, "public_answer"),
        (83, "accept_wallet_setup"),
        (84, "confirm_wallet"),
        (85, "discard_wallet"),
        (90, "lock_status"),
        (91, "set_pin"),
        (92, "unlock"),
        (93, "change_pin"),
    ];
    let table: Vec<(u16, &str)> = ops().iter().map(|o| (o.num, o.name)).collect();
    assert_eq!(table, pinned);
}

#[test]
fn every_type_an_op_mentions_is_described() {
    let text = protocol_description();
    let defined: BTreeSet<&str> = enum_tags()
        .iter()
        .map(|(t, _, _)| *t)
        .chain(
            text.lines()
                .filter(|l| !l.starts_with(' ') && l.contains(": "))
                .filter_map(|l| l.split(": ").next()),
        )
        .collect();
    for o in ops() {
        for shape in [&o.args, &o.result] {
            for name in type_names(shape) {
                assert!(
                    defined.contains(name.as_str()),
                    "{} mentions {name}",
                    o.name
                );
            }
        }
    }
}

#[test]
fn the_protocol_file_is_what_the_code_describes() {
    let path = bindings().join("PROTOCOL.md");
    let text = protocol_description();
    if std::env::var_os("OPENBMT_BLESS_PROTOCOL").is_some() {
        std::fs::create_dir_all(bindings()).expect("a directory");
        std::fs::write(&path, &text).expect("written");
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == text,
        "bindings/PROTOCOL.md differs from the code: run the tests of this crate with \
         OPENBMT_BLESS_PROTOCOL=1 to write it again"
    );
}

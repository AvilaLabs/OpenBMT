//! The description of the protocol: generated from the encoder's own tables
//! into `bindings/PROTOCOL.md`, and compared with the hand-written Kotlin.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::call::{enum_tags, ops, type_names};
use crate::{protocol_description, REQUEST_MAGIC, RESPONSE_MAGIC, VERSION};

fn bindings() -> PathBuf {
    std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("bindings")
}

#[test]
fn the_frames_have_their_own_magics() {
    assert_eq!(&REQUEST_MAGIC, b"OBMm");
    assert_eq!(&RESPONSE_MAGIC, b"OBMn");
    assert_eq!(VERSION, 1);
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
        (1, "start"),
        (2, "stop"),
        (10, "pump"),
        (11, "open"),
        (12, "judge"),
        (13, "requote"),
        (14, "cancel"),
        (15, "override_sale"),
        (16, "request_refund"),
        (17, "withdraw_refund"),
        (18, "refund_in_cash"),
        (19, "refunds"),
        (20, "heartbeat"),
        (21, "command"),
        (22, "check_in"),
        (23, "add_batch"),
        (30, "status"),
        (31, "notes"),
        (32, "own_node_retry"),
        (40, "rate_sources"),
        (41, "rate_parse"),
        (42, "rate_combine"),
        (43, "pairing_read"),
        (44, "pair_begin"),
        (45, "pair_match"),
        (46, "pair_differ"),
        (47, "pair_cancel"),
        (48, "pair_finish"),
        (49, "own_node_check"),
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

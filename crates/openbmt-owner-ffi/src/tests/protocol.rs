//! The description of the protocol: generated from the encoder's own tables
//! into `bindings/PROTOCOL.md`, and compared with the hand-written Kotlin.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::call::{enum_tags, ops, type_names};
use crate::{protocol_description, REQUEST_MAGIC, RESPONSE_MAGIC, VERSION};

fn bindings() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("bindings")
}

#[test]
fn the_frames_have_their_own_magics() {
    assert_eq!(&REQUEST_MAGIC, b"OBMk");
    assert_eq!(&RESPONSE_MAGIC, b"OBMl");
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
        (11, "link_step"),
        (12, "deliver"),
        (20, "status"),
        (21, "receipts_line"),
        (22, "merge_offer"),
        (23, "send_states"),
        (24, "old_addresses"),
        (25, "request_deep_check"),
        (30, "build_request"),
        (31, "approve_merge"),
        (32, "take"),
        (33, "sessions"),
        (34, "session_request"),
        (35, "abandon_session"),
        (40, "queue"),
        (41, "queue_refund"),
        (42, "cancel"),
        (43, "requeue"),
        (44, "cash_out"),
        (45, "sign_batch"),
        (46, "queue_settings"),
        (50, "link_state"),
        (51, "clear_alert"),
        (52, "refund_notes"),
        (53, "dismiss_refund_note"),
        (60, "notes"),
        (61, "rescan"),
        (62, "recover_key1"),
        (63, "archive_paid"),
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

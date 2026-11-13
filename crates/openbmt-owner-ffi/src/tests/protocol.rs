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
        (26, "settings"),
        (27, "outbox"),
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
        (47, "settings_preview"),
        (50, "link_state"),
        (51, "clear_alert"),
        (52, "refund_notes"),
        (53, "dismiss_refund_note"),
        (60, "notes"),
        (61, "rescan"),
        (62, "recover_key1"),
        (63, "archive_paid"),
        (70, "pos_state"),
        (71, "pos_setup"),
        (72, "pos_remove"),
        (73, "pos_request"),
        (74, "pos_reply"),
        (75, "pos_acknowledge"),
        (80, "ceremony_make_key1"),
        (81, "ceremony_read_answer"),
        (82, "ceremony_build_wallet"),
        (83, "ceremony_finish"),
        (84, "ceremony_open"),
        (85, "ceremony_stage"),
        (86, "ceremony_defaults"),
        (90, "counter_scan"),
        (91, "counter_confirm"),
        (92, "counters_waiting"),
        (93, "counter_bundle"),
        (94, "counter_delivered"),
        (95, "counter_relay"),
        (100, "setup_lock_form"),
        (101, "setup_lock"),
        (102, "setup_unlock"),
        (103, "setup_change_pin"),
        (104, "box_fetch_claim"),
        (105, "box_queue"),
        (106, "boxes_waiting"),
        (107, "box_deliver"),
        (108, "box_own_invitation"),
        (109, "box_own_check"),
        (110, "boxes_pending"),
        (111, "box_forget"),
        (120, "signer_registration"),
        (121, "signer_build"),
        (122, "signer_export"),
        (123, "signer_file"),
        (124, "signer_take"),
        (125, "first_address_record"),
        (126, "first_address_read"),
        (130, "watch_make_key"),
        (131, "watch_named"),
        (132, "watch_settings_preview"),
        (133, "watch_export"),
        (134, "watch_start"),
        (135, "watch_step"),
        (136, "watch_stop"),
        (140, "price_sources"),
        (141, "price_agree"),
        (142, "price_sources_for"),
        (143, "price_agree_for"),
        (144, "books"),
        (145, "wallet_import_read"),
        (146, "books_ledger"),
        (147, "own_owner_make"),
        (148, "own_wallet_pin"),
        (149, "own_wallet_read"),
        (150, "own_offer"),
        (151, "own_request"),
        (152, "own_confirm"),
        (153, "own_differ"),
        (154, "own_waiting"),
        (155, "own_bundle"),
        (156, "own_delivered"),
        (157, "own_gap"),
        (158, "own_gap_now"),
        (159, "own_spend_build"),
        (160, "own_spend_sessions"),
        (161, "own_spend_file"),
        (162, "own_spend_take"),
        (163, "own_spend_abandon"),
        (164, "own_merge"),
        (165, "float_open"),
        (166, "float_status"),
        (167, "float_fee_rate"),
        (168, "float_pay"),
        (169, "float_sweep"),
        (170, "float_refill"),
        (180, "starter_make"),
        (181, "starter_page"),
        (182, "starter_check_start"),
        (183, "starter_key"),
        (184, "starter_seal"),
        (185, "starter_restore_start"),
        (186, "starter_restore_finish"),
        (187, "starter_pay"),
        (188, "starter_move"),
        (189, "starter_limit_state"),
        (190, "starter_change_held"),
        (191, "starter_offers_bitcoin"),
        (192, "starter_seed_may_go"),
        (193, "starter_cancel"),
        (194, "starter_not_yet"),
        (195, "starter_fee_rate"),
        (196, "starter_open"),
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

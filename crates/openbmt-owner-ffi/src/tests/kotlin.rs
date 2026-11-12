//! Source-level checks that keep the hand-written Kotlin constants and frame
//! field order aligned with Rust. The JVM decoder behavior is exercised by
//! `bindings/tests/ProtocolDecoderTest.kt`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::wire::{frame, W};
use crate::call::{enum_tags, ops};
use crate::say::{TONES, WHOS};
use crate::{LinkConfig, OwnerNetwork, RouteView, StartConfig};

fn bindings(file: &str) -> PathBuf {
    std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("bindings")
    .join(file)
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn kotlin(file: &str) -> String {
    read(bindings(&format!("kotlin/org/openbmt/owner/{file}")))
}

/// `private const val NAME = 12` lines whose name starts with `prefix`.
fn constants(text: &str, prefix: &str) -> BTreeMap<String, u32> {
    text.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("private const val ")?;
            let (name, value) = rest.split_once(" = ")?;
            if !name.starts_with(prefix) {
                return None;
            }
            Some((name.to_string(), value.trim().parse().ok()?))
        })
        .collect()
}

/// `EntryKindView` becomes `ENTRY_KIND_VIEW`.
fn upper_snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

#[test]
fn the_kotlin_op_numbers_are_the_rust_ones() {
    let rust: BTreeMap<String, u32> = ops()
        .iter()
        .map(|o| {
            (
                format!("OP_{}", o.name.to_ascii_uppercase()),
                u32::from(o.num),
            )
        })
        .collect();
    assert_eq!(constants(&kotlin("OwnerNode.kt"), "OP_"), rust);
}

#[test]
fn the_kotlin_tag_bytes_are_the_rust_ones() {
    let rust: BTreeMap<String, u32> = enum_tags()
        .iter()
        .map(|(ty, variant, tag)| {
            (
                format!("TAG_{}_{}", upper_snake(ty), upper_snake(variant)),
                u32::from(*tag),
            )
        })
        .collect();
    assert_eq!(constants(&kotlin("OwnerNode.kt"), "TAG_"), rust);
}

#[test]
fn the_kotlin_tone_and_person_numbers_are_the_rust_ones() {
    let text = kotlin("OwnerNode.kt");
    let tones: BTreeMap<String, u32> = TONES
        .iter()
        .map(|(_, name, byte)| (format!("TONE_{}", upper_snake(name)), u32::from(*byte)))
        .collect();
    assert_eq!(constants(&text, "TONE_"), tones);
    let whos: BTreeMap<String, u32> = WHOS
        .iter()
        .map(|(_, name, byte)| (format!("WHO_{}", upper_snake(name)), u32::from(*byte)))
        .collect();
    assert_eq!(constants(&text, "WHO_"), whos);
}

fn in_order(text: &str, needles: &[&str]) {
    let positions: Vec<usize> = needles
        .iter()
        .map(|f| text.find(f).unwrap_or_else(|| panic!("{f}")))
        .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
}

#[test]
fn the_kotlin_words_read_the_fields_in_the_order_the_frame_writes_them() {
    let text = kotlin("OwnerNode.kt");
    in_order(
        &text,
        &[
            "val tone: Int",
            "val who: Int",
            "val heading: String",
            "val what: String",
            "val next: String",
            "val code: String?",
            "val at: Long?",
            "val quoted: Quoted?",
            "val detail: String?",
        ],
    );
    in_order(
        &text,
        &[
            "val tone = u8()",
            "val who = u8()",
            "val heading = string16()",
            "val what = string16()",
            "val next = string16()",
            "val code = option { string16() }",
            "val at = option { u64() }",
            "val quoted = option { Quoted(string16(), string16()) }",
            "val detail = option { string16() }",
        ],
    );
    assert!(text.contains("throw OwnerException(rule, text, status, words)"));
}

#[test]
fn the_kotlin_results_read_their_fields_in_the_order_the_rust_types_write_them() {
    let text = kotlin("OwnerNode.kt");
    // Each reader, with the fields in the order of the Rust record.
    for needle in [
        "Pump(strings(), bool())",
        "strings(), u64(), u64(), u64(), u64(), option { words() }, option { string() }, bool(),",
        "viewKind(), option { u32() }, words(), words(), u32(), u32(),",
        "option { Halted(string(), words()) }, health(), u64(), bool(), option { words() }, bool(),",
        "Health(bool(), bool(), bool(), words())",
        "Receipts(receiptsKind(), u64(), u64(), u64(), words())",
        "MergeOffer(u64(), u64(), u64(), u64(), u64(), u64(), words())",
        "SendRow(string(), sendKind(), u32(), words())",
        "Built(bytes(), bytes(), list { Skipped(u32(), string(), words()) })",
        "Outcome(u32(), outcomeKind(), u32(), option { string() }, words())",
        "Taken(bytes(), list { outcome() }, list { sendRow() })",
        "Stage(stageKind(), option { string() }, words())",
        "u32(), bytes(), u32(), string(), string(), u64(), stage(), option { bool() },",
        "CashOutRow(u32(), string(), cashAmount(), string(), stage())",
        "list { refundRow() }, list { cashOutRow() }, list { EntryRow(u32(), entryKind(), stage()) },",
        "QueuedRefund(u32(), u64(), bool())",
        "SettingsQueued(bool(), u32(), option { bytes() })",
        "Deliver(u32(), u32(), strings())",
        "Alert(alertKind(), bytes(), u64(), words())",
        "bytes(), u64(), bytes(), option { u64() }, option { bool() }, option { u64() }, words(),",
        "BatchWaiting(bytes(), u32(), u64(), words())",
        "LinkState(bool(), u64(), list { alert() }, list { counterState() }, list { batchWaiting() })",
        "RefundNote(u32(), bytes(), u64(), u64(), u32(), noteKind(), bool(), words())",
        "OldAddresses(oldKind(), u64(), u64(), option { u64() }, bool(), option { words() })",
        "DeepRequest(bool(), words())",
        "Limit(u64(), u32())",
        "CounterShare(bytes(), u32())",
        "SettingChange(string(), string(), option { bytes() }, string(), string(), bool())",
        "u32(), list { settingChange() }, bool(), bool(), u32(), option { u64() }, option { u64() }, bytes(),",
        "list { bytes() }, list { CounterShare(bytes(), u32()) }, list { bytes() },\n        option { bytes() }, shopCurrency(), option { SwapHeld(string(), string(), string(), u64()) },\n    )",
        "u32(), limit(), limit(), limit(), limit(), limit(), limit(), u32(),",
        "limit(), limit(), limit(), limit(), u8(), u8(), u32(), bool(), bool(), u64(),",
        "list { bytes() }, list { CounterShare(bytes(), u32()) }, list { bytes() },",
        "PosSetup(posVendor(), string(), strings(), strings())",
        "PosAlert(PosAlertKey(posAlertKind(), posSubject()), u64(), words())",
        "PosRequest(posMethod(), string(), bytes())",
        "PosReply(bool(), bool(), u32(), u32(), u32(), list { posAlert() }, u32(), u64())",
        "option { posSetup() }, option { u64() }, list { posAlert() }, u32(), u32(), u32(), u32(),",
        "PosSubject.Tender(string(), string())",
        "PosSubject.Sale(bytes(), u32())",
        "PartView(string(), string(), string())",
        "Key1Made(bytes(), partView(), bytes(), strings(), option { strings() }, words())",
        "AnswerView(network(), option { partView() }, partView(), bytes(), strings())",
        "bytes(), bytes(), string(), strings(), strings(), string(),",
        "list { PairView(u32(), u32(), u8(), u8()) }, words(), words(),",
        "FinishedView(bytes(), words())",
        "SetupFields(network(), bytes(), bytes(), string(), bytes(), option { string() }, u32(), u32(), u32())",
        "SetupStageView(u8(), words())",
        "CeremonyDefaults(u32(), u32(), u32())",
        "PriceSource(r.string(), r.string())",
        "AgreedPrice(r.u64(), r.u64(), r.string())",
        "OutboxRow(",
        "u32(), outboxKind(), option { string() }, option { bytes() }, option { u32() },",
        "option { u64() }, u32(), words(),",
        "OwnWalletView(bytes(), string(), bool())",
        "u32(), bytes(), list { u32() }, option { u32() }, bool(), list { signerLine() },",
        "option { signerLine() }, u64(),",
        "option { bytes() }, list { ownSpendItem() }, list { Skipped(u32(), string(), words()) },",
        "OwnSpendSession(r.bytes(), r.list { r.ownSpendItem() })",
        "OwnSpendTaken(r.u32(), r.bytes(), r.u64(), r.list { r.sendRow() })",
        "OwnMade(bytes(), u64())",
        "OwnBundleView(bytes(), bytes(), u32(), u32(), u32(), list { CounterShare(bytes(), u32()) })",
        "OwnOfferView(r.ownMade(), r.u64(), r.u32(), r.string())",
        "r.bytes(), r.string(), r.strings(), r.bool(), r.list { CounterShare(r.bytes(), r.u32()) }, r.u64(),",
        "string(a.dir).bytes(a.ownerSecret).bytes(a.wallet)",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
    // And the Rust records those readers mirror, in the same order.
    let protocol = read(bindings("PROTOCOL.md"));
    for line in [
        "PumpView: record: notes: list<string>, stopped: bool",
        "HealthView: record: known: bool, healthy: bool, behind: bool, words: Words",
        "StatusView: record: view: ViewKind, rescan_to: option<u32>, view_words: Words, receipts_words: Words, checked_height: u32, tip_height: u32, halted: option<HaltedView>, health: HealthView, balance_sats: u64, recovery_pending: bool, recovery_words: option<Words>, stopped: bool",
        "ReceiptsView: record: kind: ReceiptsKind, payments: u64, cost_sats: u64, rate_per_kvb: u64, words: Words",
        "SendRow: record: txid: string, state: SendKind, n: u32, words: Words",
        "BuiltView: record: session_id: list<u8>, request: list<u8>, skipped: list<SkippedView>",
        "TakenView: record: session_id: list<u8>, outcomes: list<OutcomeView>, sends: list<SendRow>",
        "StageView: record: kind: StageKind, detail: option<string>, words: Words",
        "QueuedRefund: record: id: u32, received_sats: u64, reached_refund_depth: bool",
        "PartView: record: fingerprint: string, origin_path: string, account_xpub: string",
        "Key1Made: record: record: list<u8>, key1: PartView, owner_root: list<u8>, owner_root_words: list<string>, words: option<list<string>>, state_words: Words",
        "AnswerView: record: network: OwnerNetwork, key2: option<PartView>, key3: PartView, key_app_root: list<u8>, key_app_root_words: list<string>",
        "FinishedView: record: record: list<u8>, state_words: Words",
        "CeremonyDefaults: record: batch_min_count: u32, refund_depth: u32, birthday: u32",
        "SetupStageView: record: stage: u8, words: Words",
        "SettingsEditArgs: record: usd_per_btc: option<u64>, light_per_sale: option<u32>, light_budget: option<u32>, node_per_sale: option<u32>, node_budget: option<u32>, box_per_sale: option<u32>, box_budget: option<u32>, underpayment_tolerance_bps: option<u32>, refund_cap: option<u32>, refund_daily_cap: option<u32>, cashier_override_daily_cap: option<u32>, manager_override_ceiling: option<u32>, min_confirmations: option<u8>, final_confirmations: option<u8>, heartbeat_max_age_secs: option<u32>, tor_on: option<bool>, customer_wifi: option<bool>, min_sale_sats: option<u64>, tips_on: option<bool>, tip_percents: option<list<u8>>, shares: list<CounterShareView>, lightning: LightningAction, lightning_text: string, currency: CurrencyAction, swap: SwapAction, swap_name: string, swap_onion: string, swap_prefix: string, swap_max_sats: u64",
        "SettingChangeView: record: field: string, label: string, counter: option<list<u8>>, old: string, new: string, loosening: bool",
        "SettingsPreviewView: record: version: u32, changes: list<SettingChangeView>, needs_key_app: bool, nothing_changed: bool, shares_total_bps: u32, usd_per_btc: option<u64>, sats_per_usd: option<u64>, settings: list<u8>, currency: ShopCurrencyView",
        "PriceSourceView: record: id: string, url: string",
        "PriceReplyView: record: source: string, body: list<u8>, fetched_at: u64",
        "PriceArgs: record: replies: list<PriceReplyView>, now: u64",
        "AgreedPriceView: record: usd_per_btc: u64, minor_per_btc: u64, source_label: string",
        "OwnWalletView: record: file: list<u8>, first_address: string, multisig: bool",
        "OwnMade: record: code: list<u8>, made_at: u64",
        "OwnOfferView: record: made: OwnMade, ends_at: u64, floor: u32, first_address: string",
        "OwnRequestView: record: counter: list<u8>, name: string, session_words: list<string>, needs_rate: bool, shares: list<CounterShareView>, seconds_left: u64",
        "OwnBundleView: record: counter: list<u8>, code: list<u8>, first_start: u32, first_end: u32, settings_version: u32, shares: list<CounterShareView>",
        "OwnShopArgs: record: dir: string, owner_secret: list<u8>, wallet: list<u8>",
        "OwnGapView: record: needed: u32, notice: bool",
        "OldAddressesView: record: kind: OldKind, count: u64, days_ago: u64, next: option<u64>, running: bool, words: option<Words>",
    ] {
        assert!(protocol.contains(line), "{line}");
    }
}

#[test]
fn the_kotlin_error_reader_rejects_undefined_status_tags_and_utf8() {
    let text = kotlin("OwnerNode.kt");
    assert!(text.contains("fun status(): Int = u8().also { if (it !in 0..4)"));
    assert!(text.contains("it !in 0..3) bad(\"a tone is not 0 to 3\")"));
    assert!(text.contains("it !in 0..6) bad(\"a person is not 0 to 6\")"));
    assert!(text.contains("onMalformedInput(CodingErrorAction.REPORT)"));
    assert!(text.contains("onUnmappableCharacter(CodingErrorAction.REPORT)"));
    assert!(text.contains("catch (_: CharacterCodingException)"));
    assert!(text.contains("the library's response is not an OBMl frame"));
    assert!(
        text.contains("'O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'l'.code.toByte()")
    );
    assert!(
        text.contains("'O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'k'.code.toByte()")
    );
}

#[test]
fn the_kotlin_native_declaration_matches_the_exported_symbol() {
    let text = kotlin("Native.kt");
    assert!(text.contains("package org.openbmt.owner\n"));
    assert!(text.contains("object Native {"));
    assert!(text.contains("System.loadLibrary(\"openbmt_owner_ffi\")"));
    assert!(text.contains("external fun call(request: ByteArray): ByteArray?"));
    // The symbol the JVM looks for: package, class and method of the
    // declaration above (no name has an underscore to escape).
    let glue = read(
        std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR")
                .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
        )
        .join("src/jni.rs"),
    );
    assert!(glue.contains("pub extern \"system\" fn Java_org_openbmt_owner_Native_call("));
}

#[test]
fn the_kotlin_wrapper_takes_no_lock_so_that_pump_and_the_screen_can_run_together() {
    for file in ["OwnerNode.kt", "Native.kt"] {
        let text = kotlin(file);
        for line in text.lines() {
            let code = line.trim_start();
            if code.starts_with("//") || code.starts_with('*') || code.starts_with("/*") {
                continue;
            }
            assert!(!code.contains("synchronized"), "{file}: {line}");
            assert!(!code.contains("@Synchronized"), "{file}: {line}");
            assert!(!code.contains("ReentrantLock"), "{file}: {line}");
        }
    }
}

#[test]
fn the_kotlin_wrapper_validates_no_address_and_reads_no_wallet_state() {
    // An address is checked in Rust, never in Kotlin: the binding only
    // passes the text on.
    let text = kotlin("OwnerNode.kt");
    for word in [
        "Regex",
        "bech32",
        "startsWith(\"bc1",
        "startsWith(\"bcrt1",
        "base58",
    ] {
        assert!(!text.contains(word), "OwnerNode.kt mentions {word}");
    }
}

const START_FRAME_HEX: &str =
    "4f424d6b01010000000000000000000100000064030100000000000000010000000101000000020100000077010000000300040000000500000006000000010100000070010000000100000061000101000100000078010000000701000000010000000809000000000000000a0000000000000000";

fn sample() -> StartConfig {
    StartConfig {
        dir: "d".to_string(),
        network: OwnerNetwork::Regtest,
        now: 1,
        owner_secret: vec![1],
        approver_root: vec![2],
        wallet: "w".to_string(),
        key1_account: vec![3],
        key3_xpub: None,
        batch_min_count: 4,
        refund_depth: 5,
        birthday: 6,
        proxy: Some("p".to_string()),
        fixed_peers: vec!["a".to_string()],
        broadcast_proxy: None,
        without_tor: true,
        link: Some(LinkConfig {
            route: RouteView::Direct {
                addr: "x".to_string(),
            },
            relay_key: vec![7],
            counters: vec![vec![8]],
            heartbeat_every_secs: 9,
            check_in_grace_secs: 10,
        }),
        own_wallet: None,
    }
}

#[test]
fn the_start_request_is_the_same_bytes_in_rust_and_in_the_kotlin_harness() {
    let sample = sample();
    let bytes = frame(1, 0, &W::new().start(&sample).0);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, START_FRAME_HEX);
    // Through the real encoder, too.
    let mut fields = Vec::new();
    crate::codec::Put::put(&sample, &mut fields);
    assert_eq!(frame(1, 0, &fields), bytes);
    // The Kotlin harness holds the same literal.
    let harness = read(bindings("tests/ProtocolDecoderTest.kt"));
    let kotlin_hex: String = harness
        .split("START_FRAME_HEX =")
        .nth(1)
        .expect("the constant")
        .split("@JvmStatic")
        .next()
        .expect("up to main")
        .split('"')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, s)| s)
        .collect();
    assert_eq!(kotlin_hex, START_FRAME_HEX);
}

const EDIT_FRAME_HEX: &str =
    "4f424d6b012f00010000000000000001e803000000000000010a0000000000013200000000000001640000000000000103000001010000000000000000010000000000000000000000000000000000000000000000000000";

#[test]
fn the_settings_edit_request_is_the_same_bytes_in_rust_and_in_the_kotlin_harness() {
    use crate::settings_edit::{CurrencyAction, LightningAction, SettingsEditArgs};
    let edit = SettingsEditArgs {
        usd_per_btc: Some(1_000),
        light_per_sale: Some(10),
        light_budget: None,
        node_per_sale: None,
        node_budget: Some(50),
        box_per_sale: None,
        box_budget: None,
        underpayment_tolerance_bps: None,
        refund_cap: Some(100),
        refund_daily_cap: None,
        cashier_override_daily_cap: None,
        manager_override_ceiling: None,
        min_confirmations: Some(3),
        final_confirmations: None,
        heartbeat_max_age_secs: None,
        tor_on: Some(true),
        customer_wifi: None,
        min_sale_sats: None,
        tips_on: None,
        tip_percents: None,
        shares: Vec::new(),
        lightning: LightningAction::Remove,
        lightning_text: String::new(),
        currency: CurrencyAction::Keep,
        swap: crate::settings_edit::SwapAction::Keep,
        swap_name: String::new(),
        swap_onion: String::new(),
        swap_prefix: String::new(),
        swap_max_sats: 0,
    };
    let mut fields = Vec::new();
    crate::codec::Put::put(&edit, &mut fields);
    let hex: String = frame(47, 1, &fields)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, EDIT_FRAME_HEX);
    // The Kotlin harness holds the same literal.
    let harness = read(bindings("tests/ProtocolDecoderTest.kt"));
    let kotlin_hex: String = harness
        .split("EDIT_FRAME_HEX =")
        .nth(1)
        .expect("the constant")
        .split("private fun sampleEdit")
        .next()
        .expect("up to the sample")
        .split('"')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, s)| s)
        .collect();
    assert_eq!(kotlin_hex, EDIT_FRAME_HEX);
}

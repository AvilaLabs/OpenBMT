//! Source-level checks that keep the hand-written Kotlin constants and frame
//! field order aligned with Rust. The JVM decoder behavior is exercised by
//! `bindings/tests/ProtocolDecoderTest.kt`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::wire::{frame, W};
use crate::call::{enum_tags, ops};
use crate::say::{TONES, WHOS};
use crate::StartConfig;

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
    read(bindings(&format!("kotlin/org/openbmt/counter/{file}")))
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

/// `RefundAskView` becomes `REFUND_ASK_VIEW`.
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
    assert_eq!(constants(&kotlin("CounterNode.kt"), "OP_"), rust);
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
    assert_eq!(constants(&kotlin("CounterNode.kt"), "TAG_"), rust);
}

#[test]
fn the_kotlin_tone_and_person_numbers_are_the_rust_ones() {
    let text = kotlin("CounterNode.kt");
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
    let text = kotlin("CounterNode.kt");
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
    assert!(text.contains("throw CounterException(rule, text, status, words)"));
}

#[test]
fn the_kotlin_results_read_their_fields_in_the_order_the_rust_types_write_them() {
    let text = kotlin("CounterNode.kt");
    // Each reader, with the fields in the order of the Rust record.
    for needle in [
        "Opened(u32(), u64(), u64(), u64(), u64(), payment())",
        "Payment(string(), string(), string(), u32(), bytes())",
        "saleState(), words(), list { words() }, option { words() }, u64(), u64(), bool(), bool(),",
        "bool(), bool(),",
        "reports(), option { payment() },",
        "Pump(reports(), strings(), bool())",
        "u32(), option { u64() }, u64(), option { u64() }, u64(), refundState(), role(), string(), u64(),",
        "HeartbeatOutcome(bool(), option { u64() }, words(), reports())",
        "u32(), u32(), option { Health(bool(), bool(), words()) },",
        "option { OwnNodeStatus(bool(), words()) }, u32s(), u64(), bool(),",
        "option { LightningStatus(bool(), words()) }",
        "LightningSale(bool(), bool(), option { words() })",
        "LnRequest(u64(), string(), lnMethod(), string(), string(), string(), option { string() })",
        "LnStep(option { lnRequest() }, option { bytes() })",
        "Report.BooksFinal(u32(), u64(), string(), u64())",
        "RateSource(string(), string(), string(), string())",
        "Sample(string(), u64(), u64())",
        "Rate(u64(), string(), u64())",
        "ShopSettings(u32(), bool(), bool(), u64(), string(), bool(), bytes().map { it.toInt() and 0xff })",
        "words(), bool(), option { u64() }, bool(), u64(), u64(), option { u64() },",
        "u32(), bool(), option { words() },",
        "LinkStep(linkView(), strings())",
        "PairingInfo(r.network(), r.shopSettings(), r.relayKind())",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
    // And the Rust records those readers mirror, in the same order.
    let protocol = read(bindings("PROTOCOL.md"));
    for line in [
        "OpenedView: record: index: u32, needs_sats: u64, quote_rate: u64, quote_from: u64, quote_until: u64, payment: Payment",
        "Payment: record: address: string, uri: string, copy_uri: string, qr_size: u32, qr_bits: list<u8>",
        "PumpView: record: reports: list<ReportView>, notes: list<string>, stopped: bool",
        "HeartbeatOutcome: record: verified: bool, sent_at: option<u64>, words: Words, reports: list<ReportView>",
        "StatusView: record: checked_height: u32, tip_height: u32, health: option<HealthView>, own_node: option<OwnNodeView>, sales: list<u32>, refused_below_minimum: u64, stopped: bool, lightning: option<LightningStatusView>",
        "LightningSaleView: record: invoice: bool, waiting: bool, unavailable: option<Words>",
        "LightningStatusView: record: ready: bool, words: Words",
        "LnRequestView: record: id: u64, host_port: string, method: LnMethod, path: string, header_name: string, header_value: string, body: option<string>",
        "LnStepView: record: next: option<LnRequestView>, record: option<list<u8>>",
        "HealthView: record: healthy: bool, behind: bool, words: Words",
        "OwnNodeView: record: usable: bool, words: Words",
        "OwnNodeCheckArgs: record: text: string, network: CounterNetwork, tor: bool",
        "StartConfig: record: dir: string, now: u64, record: list<u8>, proxy: option<string>, own_node: option<string>, lightning: option<list<u8>>, link_proxy: option<string>",
        "PairingView: record: network: CounterNetwork, settings: SettingsView, relay: RelayKind",
        "LinkView: record: words: Words, connected: bool, since: option<u64>, refused: bool, heartbeats_taken: u64, owner_records: u64, last_check_in_at: option<u64>, settings_version: u32, restart_needed: bool, settings_line: option<Words>",
        "LinkStepView: record: link: LinkView, notes: list<string>",
        "RateSourceView: record: id: string, url: string, currency: string, initial: string",
        "RateParseArgs: record: source: string, response: list<u8>, fetched_at: u64",
        "SampleView: record: source: string, fetched_at: u64, minor_per_btc: u64",
        "RateCombineArgs: record: samples: list<SampleView>, now: u64",
        "RateView: record: rate: u64, source_label: string, source_fetched_at: u64",
        "SettingsView: record: version: u32, tor_on: bool, customer_wifi: bool, min_sale_sats: u64, currency: string, tips_on: bool, tip_percents: list<u8>",
    ] {
        assert!(protocol.contains(line), "{line}");
    }
    assert!(protocol.contains(
        "ScreenView: record: state: StateView, words: Words, flags: list<Words>, check: option<Words>, \
         received_sats: u64, needs_sats: u64, stuck: bool, late: bool, reopened: bool, \
         conflict_seen: bool, reports: list<ReportView>, payment: option<Payment>, \
         lightning: option<LightningSaleView>"
    ));
    assert!(protocol.contains(
        "RefundRow: record: sale: u32, number: option<u64>, asked: u64, value_minor: option<u64>, tip_minor: u64, \
         state: RefundStateView, by_role: ActorRole, by_name: string, at: u64, words: Words"
    ));
}

#[test]
fn the_kotlin_error_reader_rejects_undefined_status_tags_and_utf8() {
    let text = kotlin("CounterNode.kt");
    assert!(text.contains("fun status(): Int = u8().also { if (it !in 0..4)"));
    assert!(text.contains("it !in 0..3) bad(\"a tone is not 0 to 3\")"));
    assert!(text.contains("it !in 0..6) bad(\"a person is not 0 to 6\")"));
    assert!(text.contains("onMalformedInput(CodingErrorAction.REPORT)"));
    assert!(text.contains("onUnmappableCharacter(CodingErrorAction.REPORT)"));
    assert!(text.contains("catch (_: CharacterCodingException)"));
    assert!(text.contains("the library's response is not an OBMn frame"));
    assert!(
        text.contains("'O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'n'.code.toByte()")
    );
    assert!(
        text.contains("'O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'm'.code.toByte()")
    );
}

#[test]
fn the_kotlin_native_declaration_matches_the_exported_symbol() {
    let text = kotlin("Native.kt");
    assert!(text.contains("package org.openbmt.counter\n"));
    assert!(text.contains("object Native {"));
    assert!(text.contains("System.loadLibrary(\"openbmt_counter_ffi\")"));
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
    assert!(glue.contains("pub extern \"system\" fn Java_org_openbmt_counter_Native_call("));
}

#[test]
fn the_kotlin_wrapper_takes_no_lock_so_that_pump_and_the_screen_can_run_together() {
    for file in ["CounterNode.kt", "Native.kt"] {
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

const START_FRAME_HEX: &str =
    "4f424d6d010100000000000000000001000000640100000000000000010000000201010000007001010000006f01010000000300";

#[test]
fn the_start_request_is_the_same_bytes_in_rust_and_in_the_kotlin_harness() {
    let sample = StartConfig {
        dir: "d".to_string(),
        now: 1,
        record: vec![2],
        proxy: Some("p".to_string()),
        own_node: Some("o".to_string()),
        lightning: Some(vec![3]),
        link_proxy: None,
    };
    let bytes = frame(1, 0, &W::new().start(&sample).0);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, START_FRAME_HEX);
    // Through the real encoder, too.
    let mut fields = Vec::new();
    crate::codec::Put::put(&sample, &mut fields);
    assert_eq!(frame(1, 0, &fields), bytes);
    // The Kotlin harness holds the same literal.
    let kotlin_hex = harness_hex("START_FRAME_HEX");
    assert_eq!(kotlin_hex, START_FRAME_HEX);
}

const LN_REPLY_FRAME_HEX: &str = "4f424d6d01330001000000000000000700000000000000c8000000\
020000007b7d";

#[test]
fn the_ln_reply_request_is_the_same_bytes_in_rust_and_in_the_kotlin_harness() {
    let bytes = frame(51, 1, &W::new().u64(7).u32(200).bytes(b"{}").0);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, LN_REPLY_FRAME_HEX);
    // The Kotlin harness holds the same bytes, in its own pieces.
    let kotlin_hex = harness_hex("LN_REPLY_FRAME_HEX");
    assert_eq!(kotlin_hex, LN_REPLY_FRAME_HEX);
}

const RATE_PARSE_HEX: &str =
    "4f424d6d0129000000000000000000060000006b72616b656e020000007b7d0900000000000000";
const RATE_COMBINE_HEX: &str =
    "4f424d6d012a00000000000000000002000000060000006b72616b656e01000000000000003a3c70\
000000000008000000636f696e626173650200000000000000523b7000000000000300000000000000";

/// The pieces of the string constant `name` of the Kotlin harness, joined.
fn harness_hex(name: &str) -> String {
    let harness = read(bindings("tests/ProtocolDecoderTest.kt"));
    harness
        .split(&format!("const val {name} ="))
        .nth(1)
        .expect("the constant")
        .split("private const val")
        .next()
        .expect("up to the next constant")
        .split("@JvmStatic")
        .next()
        .expect("up to main")
        .split('"')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, s)| s)
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn the_price_requests_are_the_same_bytes_in_rust_and_in_the_kotlin_harness() {
    let parse = frame(41, 0, &W::new().string("kraken").bytes(b"{}").u64(9).0);
    assert_eq!(hex(&parse), RATE_PARSE_HEX);
    let combine = frame(
        42,
        0,
        &W::new()
            .u32(2)
            .string("kraken")
            .u64(1)
            .u64(7_355_450)
            .string("coinbase")
            .u64(2)
            .u64(7_355_218)
            .u64(3)
            .0,
    );
    assert_eq!(hex(&combine), RATE_COMBINE_HEX);
    assert_eq!(harness_hex("RATE_PARSE_HEX"), RATE_PARSE_HEX);
    assert_eq!(harness_hex("RATE_COMBINE_HEX"), RATE_COMBINE_HEX);
}

/// The counter app's fetch limits are the library's (RT-40, RT-43, RT-10).
#[test]
fn the_counter_app_fetch_limits_are_the_rates_library_ones() {
    let path = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("../../apps/counter-android/src/model/PriceSchedule.kt");
    let text = read(path);
    let value = |name: &str| -> String {
        let line = text
            .lines()
            .find(|l| l.contains(&format!("const val {name} =")))
            .unwrap_or_else(|| panic!("no {name} in PriceSchedule.kt"));
        let v = line.split('=').nth(1).unwrap_or("").trim();
        v.trim_end_matches('L').replace(' ', "")
    };
    assert_eq!(
        value("FETCH_INTERVAL_SECS"),
        openbmt_rates::FETCH_INTERVAL_SECS.to_string()
    );
    assert_eq!(
        value("FETCH_TIMEOUT_SECS"),
        openbmt_rates::FETCH_TIMEOUT_SECS.to_string()
    );
    assert_eq!(
        value("MAX_RESPONSE_BYTES"),
        format!("{}*1024", openbmt_rates::MAX_RESPONSE_BYTES / 1024)
    );
}

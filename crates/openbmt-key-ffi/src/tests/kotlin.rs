//! Source-level checks that keep the hand-written Kotlin constants and frame
//! field order aligned with Rust. The JVM decoder behavior is exercised by
//! `bindings/tests/ProtocolDecoderTest.kt`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::call::{enum_tags, ops};
use crate::say::{TONES, WHOS};

fn kotlin(file: &str) -> String {
    let path: PathBuf = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("bindings/kotlin/org/openbmt/key")
    .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
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

/// `LineKind` becomes `LINE_KIND`.
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
    let kotlin = constants(&kotlin("KeyApp.kt"), "OP_");
    assert_eq!(kotlin, rust);
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
    let kotlin = constants(&kotlin("KeyApp.kt"), "TAG_");
    assert_eq!(kotlin, rust);
}

#[test]
fn the_kotlin_tone_and_person_numbers_are_the_rust_ones() {
    let text = kotlin("KeyApp.kt");
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

#[test]
fn the_kotlin_words_read_the_fields_in_the_order_the_frame_writes_them() {
    let text = kotlin("KeyApp.kt");
    // The order of the fields of the data class and of the reader.
    let fields = [
        "val tone: Int",
        "val who: Int",
        "val heading: String",
        "val what: String",
        "val next: String",
        "val code: String?",
        "val at: Long?",
        "val quoted: Quoted?",
        "val detail: String?",
    ];
    let positions: Vec<usize> = fields
        .iter()
        .map(|f| text.find(f).unwrap_or_else(|| panic!("{f}")))
        .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
    let reader = [
        "val tone = u8()",
        "val who = u8()",
        "val heading = string16()",
        "val what = string16()",
        "val next = string16()",
        "val code = option { string16() }",
        "val at = option { u64() }",
        "val quoted = option { Quoted(string16(), string16()) }",
        "val detail = option { string16() }",
    ];
    let positions: Vec<usize> = reader
        .iter()
        .map(|f| text.find(f).unwrap_or_else(|| panic!("{f}")))
        .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
    assert!(text.contains("throw KeyException(rule, text, status, words)"));
}

#[test]
fn the_kotlin_error_reader_rejects_undefined_status_tags_and_utf8() {
    let text = kotlin("KeyApp.kt");
    assert!(text.contains("fun status(): Int = u8().also { if (it !in 0..4)"));
    assert!(text.contains("it !in 0..3) bad(\"a tone is not 0 to 3\")"));
    assert!(text.contains("it !in 0..6) bad(\"a person is not 0 to 6\")"));
    assert!(text.contains("onMalformedInput(CodingErrorAction.REPORT)"));
    assert!(text.contains("onUnmappableCharacter(CodingErrorAction.REPORT)"));
    assert!(text.contains("catch (_: CharacterCodingException)"));
}

#[test]
fn the_kotlin_native_declaration_matches_the_exported_symbol() {
    let text = kotlin("Native.kt");
    assert!(text.contains("package org.openbmt.key\n"));
    assert!(text.contains("object Native {"));
    assert!(text.contains("System.loadLibrary(\"openbmt_key_ffi\")"));
    assert!(text.contains("external fun call(store: KeyStore, request: ByteArray): ByteArray"));
    for method in [
        "fun load(): ByteArray?",
        "fun save(bytes: ByteArray)",
        "fun random(n: Int): ByteArray",
        "fun now(): Long",
    ] {
        assert!(text.contains(method), "{method}");
    }
    // The symbol the JVM looks for: package, class and method of the
    // declaration above (no name has an underscore to escape).
    let glue = std::fs::read_to_string(
        std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR")
                .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
        )
        .join("src/jni.rs"),
    )
    .expect("readable");
    assert!(glue.contains("pub extern \"system\" fn Java_org_openbmt_key_Native_call("));
}

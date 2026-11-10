//! The hand-written Kotlin, which cannot be compiled here: its op numbers and
//! tag bytes are compared with the Rust tables, so that at least the numbers
//! it sends are the ones the library reads.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::call::{enum_tags, ops};

fn kotlin(file: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
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
    let glue = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/jni.rs"))
        .expect("readable");
    assert!(glue.contains("pub extern \"system\" fn Java_org_openbmt_key_Native_call("));
}

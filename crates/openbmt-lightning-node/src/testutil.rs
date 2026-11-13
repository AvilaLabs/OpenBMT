//! Helpers for the tests of this crate.

use std::path::PathBuf;

/// A fresh empty directory under the cache directory the guard sets as
/// `TMPDIR`, never under `/tmp` by hand.
pub fn temp_dir(name: &str) -> PathBuf {
    let base = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap()).join(".cache/openbmt/tmp"));
    let d = base.join(format!("shop-ln-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

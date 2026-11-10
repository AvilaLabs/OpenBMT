//! The bindings generator, run from this crate so that no separately
//! installed tool is needed. See `scripts/key-ffi-build.sh`.

fn main() {
    uniffi::uniffi_bindgen_main();
}

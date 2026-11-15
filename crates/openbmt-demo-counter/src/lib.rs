//! The OpenBMT web demo's counter: the counter app's own library (the framed
//! calls of `openbmt-counter-ffi`, over the counter node) compiled to
//! WebAssembly, so that every call the app's Kotlin model makes is answered by
//! the code the Android app runs.
//!
//! What the browser has no way to give, the page gives:
//!
//! 1. the driver: [`world`] is a chain simulated in memory that the node
//!    reads its events from, in place of the peers;
//! 2. the time: the page says what time it is before each call;
//! 3. the random bytes: the page's own source, through one imported function.
//!
//! No socket, thread or file on a disk is used: the node's files are kept in
//! memory (the `vfs` of `openbmt-counter-node`), and the pairing record the
//! demo counter starts from is made by [`record`].

#![deny(missing_docs)]

#[cfg(target_arch = "wasm32")]
pub mod abi;
pub mod record;
pub mod world;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use openbmt_counter_ffi::{dispatch, Registry};

static NOW: AtomicU64 = AtomicU64::new(0);

/// Tells the module what time it is, in seconds since 1970. It is the clock
/// of the node's files and of the chain, and only goes forward.
pub fn set_now(secs: u64) {
    NOW.fetch_max(secs, Ordering::SeqCst);
    openbmt_counter_node::vfs::set_now(secs);
}

/// The time the page last gave.
pub fn now() -> u64 {
    NOW.load(Ordering::SeqCst)
}

/// The registry of the module's nodes, whose driver is the simulated chain.
pub fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Registry::with_host(world::host()))
}

/// One framed request of the counter's protocol in, one framed response out.
pub fn call(request: &[u8]) -> Vec<u8> {
    dispatch(registry(), request)
}

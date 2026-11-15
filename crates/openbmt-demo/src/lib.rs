//! The OpenBMT web demo: the counter and the owner's books, compiled to
//! WebAssembly and driven by the page, against a chain simulated in memory.
//!
//! The parts are:
//!
//! 1. [`shop`]: the shop scenario, a counter, the owner's books and a
//!    customer who pays.
//! 2. [`chain`]: the simulated chain, a mempool and mined blocks.
//! 3. [`api`]: the requests and replies, as JSON.
//! 4. `abi` (WebAssembly only): the three calls the page makes into the module.
//!
//! No network, no clock and no system randomness is used: the clock is the
//! demo's own, and the seed for the customer's coins comes from the page.

#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

#[cfg(target_arch = "wasm32")]
pub mod abi;
pub mod api;
pub mod chain;
pub mod json;
pub mod pay;
pub mod rng;
pub mod shop;
pub mod store;
pub mod wallet;
pub mod words;

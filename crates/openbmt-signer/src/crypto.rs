//! The signer's own secp256k1 context. `openbmt-core` keeps its context
//! private, so the signer holds one of its own.

use std::sync::OnceLock;

use bitcoin::secp256k1::{All, Secp256k1};

/// The shared context for signing and verifying.
pub(crate) fn secp() -> &'static Secp256k1<All> {
    static CONTEXT: OnceLock<Secp256k1<All>> = OnceLock::new();
    CONTEXT.get_or_init(Secp256k1::new)
}

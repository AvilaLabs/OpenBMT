//! SK-27: the Compact SeedQR of the SeedSigner standard. For 12 words the
//! 128 bits of entropy (the 132 bits of the words without the 4 check bits)
//! are written as 16 bytes in byte mode at error correction level L, a
//! 21 x 21 code.

use openbmt_qr::{Ecc, QrCode, QrError};

use crate::mix::ENTROPY_BYTES;

/// The Compact SeedQR of 16 bytes of entropy.
pub fn compact_seedqr(entropy: &[u8; ENTROPY_BYTES]) -> Result<QrCode, QrError> {
    openbmt_qr::encode_bytes(entropy, Ecc::L)
}

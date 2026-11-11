//! The registry types for a PSBT (UR-14): `crypto-psbt`, which Sparrow and
//! the hardware wallets in use write, and `psbt`, the same bytes under the
//! name the registry gave later. Both carry a bare CBOR byte string.

use crate::cbor::{decode_bytes, encode_bytes};
use crate::encoder::Encoder;
use crate::error::UrError;
use crate::fountain::MIN_FRAGMENT_LEN;

/// The type this crate writes.
pub const TYPE_CRYPTO_PSBT: &str = "crypto-psbt";
/// The type this crate also reads.
pub const TYPE_PSBT: &str = "psbt";

/// The five bytes every PSBT starts with.
const PSBT_MAGIC: [u8; 5] = *b"psbt\xff";

/// An encoder for a PSBT, as a `crypto-psbt` UR. The smallest fragment is
/// the shortest the rules allow, so the fragment count is the fewest.
pub fn psbt_to_ur_parts(psbt: &[u8], max_fragment_len: usize) -> Result<Encoder, UrError> {
    if !psbt.starts_with(&PSBT_MAGIC) {
        return Err(UrError::Psbt("the bytes do not start with psbt and 0xff"));
    }
    Encoder::new(
        TYPE_CRYPTO_PSBT,
        &encode_bytes(psbt),
        max_fragment_len,
        MIN_FRAGMENT_LEN,
    )
}

/// The PSBT in a finished `crypto-psbt` or `psbt` UR.
pub fn ur_to_psbt(ur_type: &str, cbor: &[u8]) -> Result<Vec<u8>, UrError> {
    if ur_type != TYPE_CRYPTO_PSBT && ur_type != TYPE_PSBT {
        return Err(UrError::Psbt("the type is not crypto-psbt or psbt"));
    }
    let bytes = decode_bytes(cbor)?;
    if !bytes.starts_with(&PSBT_MAGIC) {
        return Err(UrError::Psbt("the bytes do not start with psbt and 0xff"));
    }
    Ok(bytes.to_vec())
}

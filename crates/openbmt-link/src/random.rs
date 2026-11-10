//! Randomness from the operating system, for the one-time keys, the relay's
//! challenges and the circuit tokens (LK-03).

use std::fs::File;
use std::io::{self, Read};

/// Fill `buf` from `/dev/urandom`. There is no fallback: if the read fails,
/// the caller gets the error and must not go on (LK-03).
pub fn os_random(buf: &mut [u8; 32]) -> io::Result<()> {
    File::open("/dev/urandom")?.read_exact(buf)
}

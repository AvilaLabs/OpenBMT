//! Randomness from the operating system, for the one-time keys, the relay's
//! challenges and the circuit tokens (LK-03).

use std::io;

/// Fill `buf` with the kernel's bytes, read and checked as SS-02 says. There
/// is no fallback: if the read fails or the checks refuse it, the caller gets
/// the error and must not go on (LK-03).
pub fn os_random(buf: &mut [u8; 32]) -> io::Result<()> {
    openbmt_kernel_random::system_fill(buf).map_err(io::Error::other)
}

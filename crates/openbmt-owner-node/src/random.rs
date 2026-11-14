//! The owner node's randomness: the kernel's bytes, read and checked as SS-02
//! says, for the session ids of a spend, the nonce of each connection and the
//! draw of the next address. Nothing stands in for a read that was refused.

use openbmt_net_io::{RandomError, Randomness};

/// [`Randomness`] over `openbmt_kernel_random::system_bytes`.
#[derive(Debug, Clone, Copy, Default)]
pub struct KernelRandomness;

impl Randomness for KernelRandomness {
    fn next_u64(&mut self) -> Result<u64, RandomError> {
        openbmt_kernel_random::system_bytes::<8>()
            .map(u64::from_le_bytes)
            .map_err(|_| RandomError)
    }
}

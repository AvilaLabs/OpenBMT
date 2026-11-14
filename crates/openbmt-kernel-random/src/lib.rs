//! The kernel's random bytes, read with the `getrandom` system call and
//! nothing else (`docs/START-SIMPLE-RULES.md`, SS-01). The call waits until
//! the kernel's generator is ready; flags are 0, so it is the same pool as
//! `/dev/urandom` once that is ready. No Java generator, no pool kept by this crate,
//! no file is opened.
//!
//! The checked read of SS-02 lives here too, so that every seed, key and
//! identity of the Rust side is made from the same two checked reads
//! ([`checked_read`], [`checked_fill`], [`system_fill`]).
//!
//! This crate holds the one `unsafe` block of the starter wallet's randomness:
//! a system call with a pointer and a length of a buffer that this function
//! owns.

#![deny(missing_docs)]

use std::io;

/// Fills `out` with the kernel's random bytes. A short read is read again
/// for the rest; an interrupted call is tried again; any other error is
/// returned and `out` is left as it was filled so far (callers drop it).
pub fn fill(out: &mut [u8]) -> io::Result<()> {
    let mut have = 0;
    while have < out.len() {
        let rest = out.get_mut(have..).unwrap_or_default();
        // SAFETY: the pointer and length describe the unfilled tail of `out`,
        // which this function borrows exclusively; flags 0 means block until
        // the kernel's generator is ready.
        let n = unsafe { libc::getrandom(rest.as_mut_ptr().cast(), rest.len(), 0) };
        if n > 0 {
            have += usize::try_from(n).unwrap_or(0);
        } else if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "getrandom gave no bytes",
            ));
        } else {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }
    Ok(())
}

/// 32 bytes of the kernel's randomness (SS-01).
pub fn read32() -> io::Result<[u8; 32]> {
    let mut out = [0u8; 32];
    fill(&mut out)?;
    Ok(out)
}

/// The size of one read the checks work on.
pub const READ_BYTES: usize = 32;

/// SS-01: where the 32 bytes come from. The kernel's is [`Kernel`]; a test
/// gives fixed bytes.
pub trait KernelRandom {
    /// The next 32 bytes, or `None` when there are none to give.
    fn read32(&mut self) -> Option<[u8; READ_BYTES]>;
}

/// The kernel's bytes, from the `getrandom` system call (SS-01).
pub struct Kernel;

impl KernelRandom for Kernel {
    fn read32(&mut self) -> Option<[u8; READ_BYTES]> {
        read32().ok()
    }
}

/// Why a checked read gave no bytes (SS-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The source gave no bytes.
    Unavailable,
    /// A read was all one byte: all zero, or one byte repeated.
    Weak,
    /// Two reads were the same.
    Repeat,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "the random source gave no bytes",
            Self::Weak => "the random source gave bytes that are all one value",
            Self::Repeat => "the random source gave the same bytes twice",
        })
    }
}

impl std::error::Error for Refusal {}

/// Overwrite a buffer so that the compiler keeps the write.
fn wipe(bytes: &mut [u8]) {
    bytes.fill(0);
    std::hint::black_box(&mut *bytes);
}

/// A read that is all one byte: all zero, or one byte repeated.
fn is_weak(bytes: &[u8; READ_BYTES]) -> bool {
    match bytes.first() {
        Some(first) => bytes.iter().all(|b| b == first),
        None => true,
    }
}

/// SS-02: two separate reads of the source, neither all one byte, and the
/// two different. The first is returned; the second is wiped.
pub fn checked_read(rng: &mut dyn KernelRandom) -> Result<[u8; READ_BYTES], Refusal> {
    let mut a = rng.read32().ok_or(Refusal::Unavailable)?;
    let Some(mut b) = rng.read32() else {
        wipe(&mut a);
        return Err(Refusal::Unavailable);
    };
    let verdict = if is_weak(&a) || is_weak(&b) {
        Err(Refusal::Weak)
    } else if a == b {
        Err(Refusal::Repeat)
    } else {
        Ok(())
    };
    wipe(&mut b);
    match verdict {
        Ok(()) => Ok(a),
        Err(e) => {
            wipe(&mut a);
            Err(e)
        }
    }
}

/// `out` filled from `rng`, 32 bytes at a time, each block by
/// [`checked_read`]. On a refusal `out` is wiped and nothing is kept.
pub fn checked_fill(rng: &mut dyn KernelRandom, out: &mut [u8]) -> Result<(), Refusal> {
    for chunk in out.chunks_mut(READ_BYTES) {
        match checked_read(rng) {
            Ok(mut block) => {
                let n = chunk.len();
                chunk.copy_from_slice(block.get(..n).unwrap_or_default());
                wipe(&mut block);
            }
            Err(e) => {
                wipe(out);
                return Err(e);
            }
        }
    }
    Ok(())
}

/// `out` filled from the kernel with the checks of SS-02. The one function
/// every seed, key and identity of the Rust side asks for its bytes.
pub fn system_fill(out: &mut [u8]) -> Result<(), Refusal> {
    checked_fill(&mut Kernel, out)
}

/// `N` checked bytes from the kernel.
pub fn system_bytes<const N: usize>() -> Result<[u8; N], Refusal> {
    let mut out = [0u8; N];
    system_fill(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ss_01_the_kernel_gives_32_bytes_and_two_reads_differ() {
        let a = read32().expect("the kernel is ready");
        let b = read32().expect("the kernel is ready");
        assert_ne!(a, b);
        assert_ne!(a, [0u8; 32]);
    }

    #[test]
    fn ss_01_a_large_read_is_filled_to_its_end() {
        // More than one call may be needed; the tail must not stay zero.
        let mut big = vec![0u8; 4096];
        fill(&mut big).expect("filled");
        assert!(big[4000..].iter().any(|b| *b != 0));
        assert!(big[..96].iter().any(|b| *b != 0));
    }

    /// A source that gives the blocks it was given, then none.
    struct Fixed(Vec<[u8; READ_BYTES]>);

    impl KernelRandom for Fixed {
        fn read32(&mut self) -> Option<[u8; READ_BYTES]> {
            if self.0.is_empty() {
                None
            } else {
                Some(self.0.remove(0))
            }
        }
    }

    fn good(n: u8) -> [u8; READ_BYTES] {
        std::array::from_fn(|i| u8::try_from(i).unwrap_or(0).wrapping_mul(7).wrapping_add(n))
    }

    #[test]
    fn ss_02_two_good_reads_give_the_first() {
        let mut rng = Fixed(vec![good(1), good(2)]);
        assert_eq!(checked_read(&mut rng), Ok(good(1)));
    }

    #[test]
    fn ss_02_a_weak_or_repeated_or_missing_read_is_refused() {
        let zeros = [0u8; READ_BYTES];
        let same = [9u8; READ_BYTES];
        let cases: [(Vec<[u8; READ_BYTES]>, Refusal); 6] = [
            (vec![zeros, good(2)], Refusal::Weak),
            (vec![good(1), zeros], Refusal::Weak),
            (vec![same, good(2)], Refusal::Weak),
            (vec![good(1), good(1)], Refusal::Repeat),
            (vec![good(1)], Refusal::Unavailable),
            (vec![], Refusal::Unavailable),
        ];
        for (blocks, want) in cases {
            assert_eq!(checked_read(&mut Fixed(blocks)), Err(want));
        }
    }

    #[test]
    fn ss_02_a_fill_checks_every_block_and_wipes_on_refusal() {
        let mut out = [0u8; 70];
        let mut rng = Fixed(vec![good(1), good(2), good(3), good(4), good(5), good(6)]);
        checked_fill(&mut rng, &mut out).expect("three blocks");
        assert_eq!(out[..32], good(1));
        assert_eq!(out[32..64], good(3));
        assert_eq!(out[64..], good(5)[..6]);
        // The second block is weak: nothing of the first is kept.
        let mut out = [0u8; 64];
        let mut rng = Fixed(vec![good(1), good(2), [0u8; 32], good(4)]);
        assert_eq!(checked_fill(&mut rng, &mut out), Err(Refusal::Weak));
        assert_eq!(out, [0u8; 64]);
    }

    #[test]
    fn ss_02_the_system_source_fills_checked_bytes() {
        let a = system_bytes::<64>().expect("the kernel is ready");
        let b = system_bytes::<17>().expect("the kernel is ready");
        assert_ne!(a, [0u8; 64]);
        assert_ne!(b, [0u8; 17]);
    }

    #[test]
    fn ss_01_an_empty_read_is_nothing() {
        fill(&mut []).expect("nothing to fill");
    }
}

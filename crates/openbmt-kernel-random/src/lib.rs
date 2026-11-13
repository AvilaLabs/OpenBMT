//! The kernel's random bytes, read with the `getrandom` system call and
//! nothing else (`docs/START-SIMPLE-RULES.md`, SS-01). The call waits until
//! the kernel's generator is ready; flags are 0, so it is the same pool as
//! `/dev/urandom` once that is ready. No Java generator, no pool of our own,
//! no file is opened.
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

    #[test]
    fn ss_01_an_empty_read_is_nothing() {
        fill(&mut []).expect("nothing to fill");
    }
}

//! SHA3-256 of FIPS 202, for the checksum of a Tor v3 address (NR-92).
//!
//! The rust-bitcoin hashes and the other crates the workspace uses have no
//! SHA3, and the checksum is the only use, so the function is written here
//! rather than taken from a crate. It is the Keccak-f[1600] permutation in
//! a sponge of rate 136 bytes with the suffix bits `01` and the padding
//! `pad10*1`. It is checked against the vectors of FIPS 202 in the tests.

/// Bytes of one block of input: 1600 bits less twice the 256-bit output.
const RATE: usize = 136;

const ROUNDS: usize = 24;

/// The round constants of Keccak-f[1600].
const RC: [u64; ROUNDS] = [
    0x0000_0000_0000_0001,
    0x0000_0000_0000_8082,
    0x8000_0000_0000_808a,
    0x8000_0000_8000_8000,
    0x0000_0000_0000_808b,
    0x0000_0000_8000_0001,
    0x8000_0000_8000_8081,
    0x8000_0000_0000_8009,
    0x0000_0000_0000_008a,
    0x0000_0000_0000_0088,
    0x0000_0000_8000_8009,
    0x0000_0000_8000_000a,
    0x0000_0000_8000_808b,
    0x8000_0000_0000_008b,
    0x8000_0000_0000_8089,
    0x8000_0000_0000_8003,
    0x8000_0000_0000_8002,
    0x8000_0000_0000_0080,
    0x0000_0000_0000_800a,
    0x8000_0000_8000_000a,
    0x8000_0000_8000_8081,
    0x8000_0000_0000_8080,
    0x0000_0000_8000_0001,
    0x8000_0000_8000_8008,
];

/// The rotation of each lane in the rho step, by lane `x + 5 * y`.
const ROT: [u32; 25] = [
    0, 1, 62, 28, 27, 36, 44, 6, 55, 20, 3, 10, 43, 25, 39, 41, 45, 15, 21, 8, 18, 2, 61, 56, 14,
];

/// The state is 25 lanes of 64 bits, lane `x + 5 * y`.
fn keccak_f(a: &mut [u64; 25]) {
    for rc in RC {
        // theta
        let mut c = [0u64; 5];
        for (x, cx) in c.iter_mut().enumerate() {
            *cx = (0..5).fold(0, |acc, y| acc ^ a[x + 5 * y]);
        }
        for x in 0..5 {
            let d = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
            for y in 0..5 {
                a[x + 5 * y] ^= d;
            }
        }
        // rho and pi
        let mut b = [0u64; 25];
        for x in 0..5 {
            for y in 0..5 {
                let to = y + 5 * ((2 * x + 3 * y) % 5);
                b[to] = a[x + 5 * y].rotate_left(ROT[x + 5 * y]);
            }
        }
        // chi
        for y in 0..5 {
            for x in 0..5 {
                a[x + 5 * y] = b[x + 5 * y] ^ (!b[(x + 1) % 5 + 5 * y] & b[(x + 2) % 5 + 5 * y]);
            }
        }
        // iota
        a[0] ^= rc;
    }
}

/// XOR a block into the state, then permute.
fn absorb(state: &mut [u64; 25], block: &[u8; RATE]) {
    let (words, _) = block.as_chunks::<8>();
    for (lane, word) in state.iter_mut().zip(words) {
        *lane ^= u64::from_le_bytes(*word);
    }
    keccak_f(state);
}

/// SHA3-256 of the input.
pub fn sha3_256(input: &[u8]) -> [u8; 32] {
    let mut state = [0u64; 25];
    let (full, rest) = input.as_chunks::<RATE>();
    for block in full {
        absorb(&mut state, block);
    }
    // The last block: what is left, the suffix 01 and the 1 bit of the
    // padding after it, and the last 1 bit of the padding at the end.
    let mut last = [0u8; RATE];
    for (to, from) in last.iter_mut().zip(rest) {
        *to = *from;
    }
    last[rest.len()] ^= 0x06;
    last[RATE - 1] ^= 0x80;
    absorb(&mut state, &last);
    let mut out = [0u8; 32];
    let (words, _) = out.as_chunks_mut::<8>();
    for (word, lane) in words.iter_mut().zip(state) {
        *word = lane.to_le_bytes();
    }
    out
}

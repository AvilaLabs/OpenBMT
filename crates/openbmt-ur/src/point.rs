//! Whether 33 bytes are a compressed public key of the secp256k1 curve
//! (UR-26): the prefix 02 or 03, an x below the field prime, and an x for
//! which `x^3 + 7` has a square root, found by raising it to `(p - 1) / 2`.
//! The arithmetic is 256-bit numbers in four limbs and plain, because a key
//! is checked once per code.

/// A 256-bit number as four 64-bit limbs, least significant first.
pub(crate) type U256 = [u64; 4];

/// The field prime, `2^256 - 2^32 - 977`.
pub(crate) const P: U256 = [0xFFFF_FFFE_FFFF_FC2F, u64::MAX, u64::MAX, u64::MAX];
/// `(P - 1) / 2`, the exponent that tells a square from a non-square.
const HALF: U256 = [
    0xFFFF_FFFF_7FFF_FE17,
    u64::MAX,
    u64::MAX,
    0x7FFF_FFFF_FFFF_FFFF,
];
const ZERO: U256 = [0; 4];
const ONE: U256 = [1, 0, 0, 0];
const SEVEN: U256 = [7, 0, 0, 0];

fn from_be(bytes: &[u8]) -> Option<U256> {
    let bytes: &[u8; 32] = bytes.try_into().ok()?;
    let mut out = ZERO;
    for (limb, chunk) in out.iter_mut().rev().zip(bytes.as_chunks::<8>().0) {
        *limb = u64::from_be_bytes(*chunk);
    }
    Some(out)
}

/// Whether `a >= b`.
fn at_least(a: &U256, b: &U256) -> bool {
    a.iter().rev().cmp(b.iter().rev()) != std::cmp::Ordering::Less
}

/// `a + b` modulo 2^256, and whether it overflowed.
fn add_raw(a: &U256, b: &U256) -> (U256, bool) {
    let mut out = ZERO;
    let mut carry = false;
    for ((o, x), y) in out.iter_mut().zip(a).zip(b) {
        let (s1, c1) = x.overflowing_add(*y);
        let (s2, c2) = s1.overflowing_add(u64::from(carry));
        *o = s2;
        carry = c1 || c2;
    }
    (out, carry)
}

/// `a - b` modulo 2^256.
fn sub_raw(a: &U256, b: &U256) -> U256 {
    let mut out = ZERO;
    let mut borrow = false;
    for ((o, x), y) in out.iter_mut().zip(a).zip(b) {
        let (s1, b1) = x.overflowing_sub(*y);
        let (s2, b2) = s1.overflowing_sub(u64::from(borrow));
        *o = s2;
        borrow = b1 || b2;
    }
    out
}

/// `a + b` modulo the prime, for `a` and `b` below it.
fn add_mod(a: &U256, b: &U256) -> U256 {
    let (sum, overflow) = add_raw(a, b);
    if overflow || at_least(&sum, &P) {
        sub_raw(&sum, &P)
    } else {
        sum
    }
}

/// `2^256` modulo the prime: `2^32 + 977`.
const C: u64 = 0x1_0000_03D1;

/// The 512-bit product of `a` and `b`, least significant limb first.
fn mul_wide(a: &U256, b: &U256) -> [u64; 8] {
    let mut out = [0u64; 8];
    for (i, x) in a.iter().enumerate() {
        let mut carry = 0u128;
        let mut slots = out.iter_mut().skip(i);
        for y in b {
            if let Some(slot) = slots.next() {
                let t = u128::from(*x) * u128::from(*y) + u128::from(*slot) + carry;
                *slot = t as u64;
                carry = t >> 64;
            }
        }
        // The slot above this row has not been written yet.
        if let Some(slot) = slots.next() {
            *slot = carry as u64;
        }
    }
    out
}

/// `lo + hi * C` in five limbs.
fn fold(lo: &U256, hi: &U256) -> [u64; 5] {
    let mut out = [0u64; 5];
    let mut carry = 0u128;
    for ((o, l), h) in out.iter_mut().zip(lo).zip(hi) {
        let t = u128::from(*l) + u128::from(*h) * u128::from(C) + carry;
        *o = t as u64;
        carry = t >> 64;
    }
    if let Some(top) = out.get_mut(4) {
        *top = carry as u64;
    }
    out
}

/// `a * b` modulo the prime: the 512-bit product, with `2^256` replaced by
/// `2^32 + 977` twice, and the prime taken off once at the end.
pub(crate) fn mul_mod(a: &U256, b: &U256) -> U256 {
    let wide = mul_wide(a, b);
    let (lo, hi) = wide.split_at(4);
    let lo: U256 = lo.try_into().unwrap_or(ZERO);
    let hi: U256 = hi.try_into().unwrap_or(ZERO);
    let [a0, a1, a2, a3, top] = fold(&lo, &hi);
    let [b0, b1, b2, b3, over] = fold(&[a0, a1, a2, a3], &[top, 0, 0, 0]);
    let mut r = [b0, b1, b2, b3];
    if over != 0 {
        r = add_raw(&r, &[C, 0, 0, 0]).0;
    }
    if at_least(&r, &P) {
        r = sub_raw(&r, &P);
    }
    r
}

/// `a ^ e` modulo the prime.
fn pow_mod(a: &U256, e: &U256) -> U256 {
    let mut acc = ONE;
    for limb in e.iter().rev() {
        for bit in (0..64).rev() {
            acc = mul_mod(&acc, &acc);
            if (limb >> bit) & 1 == 1 {
                acc = mul_mod(&acc, a);
            }
        }
    }
    acc
}

/// Whether `key` is a point of the curve in the 33-byte compressed form.
pub(crate) fn is_compressed_point(key: &[u8]) -> bool {
    let Some((&prefix, x)) = key.split_first() else {
        return false;
    };
    if prefix != 2 && prefix != 3 {
        return false;
    }
    let Some(x) = from_be(x) else {
        return false;
    };
    if at_least(&x, &P) {
        return false;
    }
    let y2 = add_mod(&mul_mod(&mul_mod(&x, &x), &x), &SEVEN);
    pow_mod(&y2, &HALF) == ONE
}

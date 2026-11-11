//! Reed-Solomon over GF(256) with the polynomial 0x11D.

/// Product in GF(256).
pub(crate) fn mul(x: u8, y: u8) -> u8 {
    let mut z: u16 = 0;
    for i in (0..8).rev() {
        z = (z << 1) ^ ((z >> 7) * 0x11D);
        z ^= u16::from((y >> i) & 1) * u16::from(x);
    }
    (z & 0xFF) as u8
}

/// The generator polynomial of the given degree without its leading 1,
/// highest power first: the product of (x - 2^i) for i below the degree.
pub(crate) fn divisor(degree: usize) -> Vec<u8> {
    let mut result = vec![0u8; degree];
    if let Some(last) = result.last_mut() {
        *last = 1;
    }
    let mut root = 1u8;
    for _ in 0..degree {
        for j in 0..degree {
            let next = result.get(j + 1).copied().unwrap_or(0);
            if let Some(c) = result.get_mut(j) {
                *c = mul(*c, root) ^ next;
            }
        }
        root = mul(root, 2);
    }
    result
}

/// The remainder of `data` divided by the generator: the error-correction
/// codewords of one block.
pub(crate) fn remainder(data: &[u8], divisor: &[u8]) -> Vec<u8> {
    let mut result = vec![0u8; divisor.len()];
    for &b in data {
        let factor = b ^ result.first().copied().unwrap_or(0);
        if !result.is_empty() {
            result.remove(0);
        }
        result.push(0);
        for (x, &y) in result.iter_mut().zip(divisor) {
            *x ^= mul(y, factor);
        }
    }
    result
}

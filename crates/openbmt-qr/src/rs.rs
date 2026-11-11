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

/// Powers of 2 and their logarithms in GF(256), from the same polynomial.
const fn build_tables() -> ([u8; 512], [u8; 256]) {
    let mut exp = [0u8; 512];
    let mut log = [0u8; 256];
    let mut x: u16 = 1;
    let mut i = 0;
    while i < 255 {
        exp[i] = x as u8;
        log[x as usize] = i as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11D;
        }
        i += 1;
    }
    while i < 512 {
        exp[i] = exp[i - 255];
        i += 1;
    }
    (exp, log)
}

const TABLES: ([u8; 512], [u8; 256]) = build_tables();

/// 2 to the power `i`.
fn exp(i: usize) -> u8 {
    TABLES.0.get(i).copied().unwrap_or(0)
}

fn log(a: u8) -> usize {
    TABLES.1.get(usize::from(a)).copied().map_or(0, usize::from)
}

fn fast_mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        0
    } else {
        exp(log(a) + log(b))
    }
}

/// `a / b`, with `b` not zero.
fn fast_div(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        0
    } else {
        exp(log(a) + 255 - log(b))
    }
}

/// The value of a polynomial (lowest power first) at `x`.
fn eval_low_first(poly: &[u8], x: u8) -> u8 {
    poly.iter().rev().fold(0u8, |acc, &c| fast_mul(acc, x) ^ c)
}

/// The syndromes of a block (data then error-correction codewords): the
/// values of the block as a polynomial, first codeword highest, at 2^0 to
/// 2^(ecc_len - 1).
fn syndromes(block: &[u8], ecc_len: usize) -> Vec<u8> {
    (0..ecc_len)
        .map(|j| {
            let a = exp(j);
            block.iter().fold(0u8, |acc, &c| fast_mul(acc, a) ^ c)
        })
        .collect()
}

/// Corrects a block in place: Berlekamp-Massey for the locator, Chien
/// search for the places, Forney for the values. Returns the number of
/// codewords changed, or `None` when there are more errors than
/// `ecc_len / 2` or the result is not a codeword.
pub(crate) fn correct(block: &mut [u8], ecc_len: usize) -> Option<usize> {
    let n = block.len();
    if ecc_len == 0 || n <= ecc_len || n > 255 {
        return None;
    }
    let synd = syndromes(block, ecc_len);
    if synd.iter().all(|&s| s == 0) {
        return Some(0);
    }
    let mut lambda: Vec<u8> = vec![1];
    let mut prev: Vec<u8> = vec![1];
    let mut l = 0usize;
    let mut m = 1usize;
    let mut b = 1u8;
    for i in 0..ecc_len {
        let mut d = synd.get(i).copied().unwrap_or(0);
        for k in 1..=l {
            let lk = lambda.get(k).copied().unwrap_or(0);
            let sk = synd.get(i.saturating_sub(k)).copied().unwrap_or(0);
            d ^= fast_mul(lk, sk);
        }
        if d == 0 {
            m += 1;
            continue;
        }
        let before = lambda.clone();
        let coef = fast_div(d, b);
        if lambda.len() < prev.len() + m {
            lambda.resize(prev.len() + m, 0);
        }
        for (k, &p) in prev.iter().enumerate() {
            if let Some(slot) = lambda.get_mut(k + m) {
                *slot ^= fast_mul(coef, p);
            }
        }
        if 2 * l <= i {
            l = i + 1 - l;
            prev = before;
            b = d;
            m = 1;
        } else {
            m += 1;
        }
    }
    if l == 0 || l > ecc_len / 2 {
        return None;
    }
    lambda.resize(l + 1, 0);
    // Chien search: an error at power p of x makes 2^-p a root.
    let mut places: Vec<usize> = Vec::new();
    for p in 0..n {
        let x = exp((255 - p % 255) % 255);
        if eval_low_first(&lambda, x) == 0 {
            places.push(p);
        }
    }
    if places.len() != l {
        return None;
    }
    // Omega = synd * lambda mod x^ecc_len.
    let mut omega = vec![0u8; ecc_len];
    for (i, &s) in synd.iter().enumerate() {
        for (j, &lj) in lambda.iter().enumerate() {
            if let Some(slot) = omega.get_mut(i + j) {
                *slot ^= fast_mul(s, lj);
            }
        }
    }
    // The formal derivative of lambda: odd terms, one power lower.
    let deriv: Vec<u8> = lambda
        .iter()
        .enumerate()
        .skip(1)
        .map(|(k, &c)| if k % 2 == 1 { c } else { 0 })
        .collect();
    for &p in &places {
        let x_inv = exp((255 - p % 255) % 255);
        let den = eval_low_first(&deriv, x_inv);
        if den == 0 {
            return None;
        }
        let value = fast_mul(exp(p % 255), fast_div(eval_low_first(&omega, x_inv), den));
        let at = n - 1 - p;
        if let Some(c) = block.get_mut(at) {
            *c ^= value;
        }
    }
    if syndromes(block, ecc_len).iter().any(|&s| s != 0) {
        return None;
    }
    Some(l)
}

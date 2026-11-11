//! Base58Check, for the 78-byte extended public keys of BIP 32 (UR-28).
//! Only what the key text needs: both directions, strict.

use crate::hash::sha256;

const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// The longest payload this module handles: an extended key.
const MAX_PAYLOAD: usize = 78;

fn checksum(payload: &[u8]) -> [u8; 4] {
    let h = sha256(&sha256(payload));
    let mut out = [0u8; 4];
    for (o, b) in out.iter_mut().zip(h.iter()) {
        *o = *b;
    }
    out
}

/// `payload` and its four checksum bytes in base58. Nothing is written for
/// a payload over [`MAX_PAYLOAD`] bytes.
pub(crate) fn encode_check(payload: &[u8]) -> Option<String> {
    if payload.len() > MAX_PAYLOAD {
        return None;
    }
    let mut data = payload.to_vec();
    data.extend_from_slice(&checksum(payload));
    let zeros = data.iter().take_while(|&&b| b == 0).count();
    // Repeated division of the number by 58; the remainders are the digits.
    let mut digits: Vec<u8> = Vec::new();
    let mut number: Vec<u8> = data.iter().copied().skip(zeros).collect();
    while !number.is_empty() {
        let mut rem = 0u32;
        let mut next: Vec<u8> = Vec::with_capacity(number.len());
        for &b in &number {
            let acc = (rem << 8) | u32::from(b);
            let q = acc / 58;
            rem = acc % 58;
            if !next.is_empty() || q != 0 {
                next.push(u8::try_from(q).ok()?);
            }
        }
        digits.push(u8::try_from(rem).ok()?);
        number = next;
    }
    let mut out = String::new();
    for _ in 0..zeros {
        out.push('1');
    }
    for d in digits.iter().rev() {
        out.push(char::from(*ALPHABET.get(usize::from(*d))?));
    }
    Some(out)
}

/// The payload of a base58 text whose last four bytes are the checksum of
/// the rest, or `None`: a character outside the alphabet, a text that is
/// too long, a wrong checksum.
pub(crate) fn decode_check(text: &str) -> Option<Vec<u8>> {
    // 82 bytes (payload and checksum) are at most 113 characters.
    if text.is_empty() || text.len() > 113 || !text.is_ascii() {
        return None;
    }
    let zeros = text.bytes().take_while(|&c| c == b'1').count();
    // Little-endian bytes of the number so far.
    let mut number: Vec<u8> = Vec::new();
    for c in text.bytes().skip(zeros) {
        let digit = ALPHABET.iter().position(|&a| a == c)?;
        let mut carry = u32::try_from(digit).ok()?;
        for b in number.iter_mut() {
            let acc = u32::from(*b) * 58 + carry;
            *b = (acc & 0xff) as u8;
            carry = acc >> 8;
        }
        while carry > 0 {
            number.push((carry & 0xff) as u8);
            carry >>= 8;
        }
        if number.len() > MAX_PAYLOAD + 4 {
            return None;
        }
    }
    let mut data = vec![0u8; zeros];
    data.extend(number.iter().rev());
    if data.len() < 4 || data.len() > MAX_PAYLOAD + 4 {
        return None;
    }
    let (payload, sum) = data.split_at(data.len() - 4);
    if checksum(payload).as_slice() != sum {
        return None;
    }
    Some(payload.to_vec())
}

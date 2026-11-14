//! The fountain code of BCR-2020-005 (UR-15): the choice of the fragment
//! length, the cutting of a message into padded fragments, which fragments
//! a part mixes (from its sequence number and the message checksum), and
//! the CBOR body of a part.

use crate::cbor::{push_head, read_head, MAJOR_ARRAY, MAJOR_BYTES, MAJOR_UINT};
use crate::error::UrError;
use crate::xoshiro::Xoshiro256;

/// UR-02: the most bytes a message may have.
pub const MAX_MESSAGE_LEN: usize = 1 << 20;
/// UR-03: the most parts one message is cut into.
pub const MAX_SEQ_LEN: usize = 10_000;
/// UR-04: the shortest fragment of a multi-part message.
pub const MIN_FRAGMENT_LEN: usize = 10;
/// UR-04: the longest fragment.
pub const MAX_FRAGMENT_LEN: usize = 2000;

/// The fragment length for a message: the smallest number of fragments
/// (up to `message_len / min`) for which the fragment length fits `max`,
/// then the length that many fragments need.
pub(crate) fn nominal_fragment_len(message_len: usize, min: usize, max: usize) -> usize {
    let most = (message_len / min.max(1)).max(1);
    let mut len = message_len;
    for count in 1..=most {
        len = message_len.div_ceil(count);
        if len <= max {
            break;
        }
    }
    len
}

/// The message in fragments of `fragment_len`, the last padded with zeros.
pub(crate) fn partition(message: &[u8], fragment_len: usize) -> Vec<Vec<u8>> {
    message
        .chunks(fragment_len.max(1))
        .map(|c| {
            let mut f = c.to_vec();
            f.resize(fragment_len, 0);
            f
        })
        .collect()
}

/// Walker's alias method as Vose gave it, over the weights 1, 1/2, ... 1/n.
#[derive(Debug, Clone)]
struct Sampler {
    probs: Vec<f64>,
    aliases: Vec<usize>,
}

impl Sampler {
    fn new(weights: &[f64]) -> Self {
        let n = weights.len();
        let sum: f64 = weights.iter().sum();
        let mut p: Vec<f64> = weights.iter().map(|w| w * n as f64 / sum).collect();
        let mut small = Vec::new();
        let mut large = Vec::new();
        for (i, &v) in p.iter().enumerate().rev() {
            if v < 1.0 {
                small.push(i);
            } else {
                large.push(i);
            }
        }
        let mut probs = vec![0.0; n];
        let mut aliases = vec![0usize; n];
        while !small.is_empty() && !large.is_empty() {
            let (Some(a), Some(g)) = (small.pop(), large.pop()) else {
                break;
            };
            let pa = p.get(a).copied().unwrap_or(1.0);
            let pg = p.get(g).copied().unwrap_or(1.0);
            if let Some(slot) = probs.get_mut(a) {
                *slot = pa;
            }
            if let Some(slot) = aliases.get_mut(a) {
                *slot = g;
            }
            let rest = (pg + pa) - 1.0;
            if let Some(slot) = p.get_mut(g) {
                *slot = rest;
            }
            if rest < 1.0 {
                small.push(g);
            } else {
                large.push(g);
            }
        }
        for i in large.into_iter().chain(small) {
            if let Some(slot) = probs.get_mut(i) {
                *slot = 1.0;
            }
        }
        Self { probs, aliases }
    }

    fn sample(&self, rng: &mut Xoshiro256) -> usize {
        let r1 = rng.next_double();
        let r2 = rng.next_double();
        let i = (self.probs.len() as f64 * r1) as usize;
        let keep = self.probs.get(i).copied().unwrap_or(1.0);
        if r2 < keep {
            i
        } else {
            self.aliases.get(i).copied().unwrap_or(i)
        }
    }
}

/// The sampler of the degree of a mixed part: 1 to `seq_len`, with weight
/// 1/degree.
fn degree_sampler(seq_len: usize) -> Sampler {
    let weights: Vec<f64> = (1..=seq_len).map(|i| 1.0 / i as f64).collect();
    Sampler::new(&weights)
}

/// Chooses the fragments of parts for one sequence length and checksum.
/// The degree sampler and the counting tree are built once, so a part
/// costs `degree * log(seq_len)` and not a rebuild of `seq_len` entries.
#[derive(Debug, Clone)]
pub(crate) struct Chooser {
    seq_len: usize,
    checksum: u32,
    sampler: Sampler,
    /// A Fenwick tree over `seq_len` ones: the fresh state of a shuffle.
    fresh: Vec<u32>,
}

impl Chooser {
    pub(crate) fn new(seq_len: usize, checksum: u32) -> Self {
        let fresh = (1..=seq_len)
            .map(|i| (i & i.wrapping_neg()) as u32)
            .collect();
        Self {
            seq_len,
            checksum,
            sampler: degree_sampler(seq_len),
            fresh,
        }
    }

    /// The fragments (their indexes, ascending) that part `seq_num` is made
    /// of. Parts 1 to `seq_len` are the fragments one by one; later parts
    /// are seeded from the sequence number and the checksum, both four
    /// bytes big-endian.
    pub(crate) fn choose(&self, seq_num: u32) -> Vec<usize> {
        let seq_len = self.seq_len;
        if seq_len == 0 {
            return Vec::new();
        }
        if usize::try_from(seq_num).is_ok_and(|n| n <= seq_len) {
            return vec![usize::try_from(seq_num).unwrap_or(1).saturating_sub(1)];
        }
        let mut seed = [0u8; 8];
        if let Some((a, b)) = seed.split_at_mut_checked(4) {
            a.copy_from_slice(&seq_num.to_be_bytes());
            b.copy_from_slice(&self.checksum.to_be_bytes());
        }
        let mut rng = Xoshiro256::from_bytes(&seed);
        let degree = self.sampler.sample(&mut rng) + 1;
        // A partial shuffle: the first `degree` draws of the full one, each
        // taking the `at`-th of the fragments not yet taken.
        let mut tree = self.fresh.clone();
        let mut chosen = Vec::with_capacity(degree);
        for left in (seq_len - degree + 1..=seq_len).rev() {
            let at = rng.next_int(0, (left as u64).saturating_sub(1)) as usize;
            let index = take_nth(&mut tree, at);
            chosen.push(index);
        }
        chosen.sort_unstable();
        chosen
    }
}

/// Removes the `at`-th (from 0) entry still present in the Fenwick tree of
/// ones and gives its position.
fn take_nth(tree: &mut [u32], at: usize) -> usize {
    let n = tree.len();
    let mut pos = 0usize;
    let mut rest = at as u32;
    let mut step = n.checked_next_power_of_two().unwrap_or(1);
    while step > 0 {
        let next = pos + step;
        if let Some(&count) = next.checked_sub(1).and_then(|i| tree.get(i)) {
            if count <= rest {
                pos = next;
                rest -= count;
            }
        }
        step >>= 1;
    }
    // `pos` entries precede the one taken, which is at position `pos`.
    let mut i = pos + 1;
    while let Some(slot) = i.checked_sub(1).and_then(|k| tree.get_mut(k)) {
        *slot = slot.saturating_sub(1);
        i += i & i.wrapping_neg();
    }
    pos
}

/// The fragments of part `seq_num`, for one-off use (the tests).
#[cfg(test)]
pub(crate) fn choose_fragments(seq_num: u32, seq_len: usize, checksum: u32) -> Vec<usize> {
    Chooser::new(seq_len, checksum).choose(seq_num)
}

/// `into` made the exclusive or with `other` (the shorter is padded).
pub(crate) fn xor_into(into: &mut [u8], other: &[u8]) {
    for (a, b) in into.iter_mut().zip(other) {
        *a ^= b;
    }
}

/// One part, as the CBOR array `[seqNum, seqLen, messageLen, checksum,
/// data]`.
pub(crate) fn encode_part(
    seq_num: u32,
    seq_len: usize,
    message_len: usize,
    checksum: u32,
    data: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 24);
    push_head(&mut out, MAJOR_ARRAY, 5);
    push_head(&mut out, MAJOR_UINT, u64::from(seq_num));
    push_head(&mut out, MAJOR_UINT, seq_len as u64);
    push_head(&mut out, MAJOR_UINT, message_len as u64);
    push_head(&mut out, MAJOR_UINT, u64::from(checksum));
    push_head(&mut out, MAJOR_BYTES, data.len() as u64);
    out.extend_from_slice(data);
    out
}

/// The fields of a part, as read (before any bound is applied).
pub(crate) struct RawPart<'a> {
    pub seq_num: u64,
    pub seq_len: u64,
    pub message_len: u64,
    pub checksum: u64,
    pub data: &'a [u8],
}

/// Reads a part body: exactly the five-item array in the shortest form,
/// with nothing after it (UR-08).
pub(crate) fn decode_part(body: &[u8]) -> Result<RawPart<'_>, UrError> {
    const CUT: UrError = UrError::Envelope("a head is cut, indefinite or not shortest");
    let Some((major, count, rest)) = read_head(body) else {
        return Err(CUT);
    };
    if major != MAJOR_ARRAY || count != 5 {
        return Err(UrError::Envelope("the body is not an array of five items"));
    }
    let mut rest = rest;
    let mut nums = [0u64; 4];
    for slot in nums.iter_mut() {
        let Some((m, v, r)) = read_head(rest) else {
            return Err(CUT);
        };
        if m != MAJOR_UINT {
            return Err(UrError::Envelope(
                "one of the first four items is not a number",
            ));
        }
        *slot = v;
        rest = r;
    }
    let Some((m, len, rest)) = read_head(rest) else {
        return Err(CUT);
    };
    if m != MAJOR_BYTES {
        return Err(UrError::Envelope("the fifth item is not a byte string"));
    }
    let Ok(len) = usize::try_from(len) else {
        return Err(UrError::Envelope("the data length does not fit"));
    };
    if rest.len() != len {
        return Err(UrError::Envelope("the data length does not match the body"));
    }
    let [seq_num, seq_len, message_len, checksum] = nums;
    Ok(RawPart {
        seq_num,
        seq_len,
        message_len,
        checksum,
        data: rest,
    })
}

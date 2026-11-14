//! The decoder: parts in any order, with repeats and losses, to the
//! message. It never panics on any text; every refusal leaves what it had.

use std::collections::BTreeSet;

use crate::bytewords::decode_minimal;
use crate::encoder::check_type;
use crate::error::UrError;
use crate::fountain::{
    decode_part, xor_into, Chooser, MAX_FRAGMENT_LEN, MAX_MESSAGE_LEN, MAX_SEQ_LEN,
    MIN_FRAGMENT_LEN,
};
use crate::hash::crc32;

/// UR-16: the longest text that can be a UR within the bounds: `ur:`, a
/// type, a sequence and the Bytewords of a whole 1 MiB message in one part.
pub const MAX_TEXT_LEN: usize = 3 + 64 + 1 + 2 * (MAX_MESSAGE_LEN + 4 + 16) + 32;

/// How far a decoder has come.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub complete: bool,
    /// Distinct parts accepted so far (1 for a single-part UR).
    pub received_parts: usize,
    /// The sequence length: parts that carry the message once.
    pub expected_parts: usize,
    /// 0.0 to 1.0, an estimate: 1.0 only when complete.
    pub estimated_fraction: f64,
}

/// UR-11: the index entries all held mixed parts may carry together, per
/// fragment of the sequence. An entry is 8 bytes in the part and 4 in the
/// watch list of its fragment, so at the largest sequence length (10,000)
/// the budget of 1,000,000 entries is 12 MB whatever the parts' degrees.
/// An honest sequence of that length holds parts of degree 100 on average.
const HELD_ENTRIES_PER_FRAGMENT: usize = 100;

/// UR-11: a mixed part with more fragments than half the sequence length
/// (but at least this many) is not held. Such parts are rare in an honest
/// sequence and the fragments one by one come anyway.
const MIN_HELD_DEGREE: usize = 64;

/// A part that is a mix of several fragments. Its `indexes` are the ones
/// not known when it came; `left` counts those still unknown.
struct Mixed {
    indexes: Vec<usize>,
    data: Vec<u8>,
    left: usize,
}

/// What the first accepted part fixed, and what has come since.
struct State {
    ur_type: String,
    seq_len: usize,
    message_len: usize,
    checksum: u32,
    fragment_len: usize,
    known: Vec<Option<Vec<u8>>>,
    chooser: Chooser,
    /// Held mixed parts by slot; a freed slot is `None` and listed in `free`.
    mixed: Vec<Option<Mixed>>,
    free: Vec<usize>,
    /// For each fragment, the slots of the held parts that carry it.
    watch: Vec<Vec<u32>>,
    held: usize,
    /// `(fragments still unknown, slot)` of every held part, to find the
    /// one that is furthest from known.
    by_left: BTreeSet<(usize, u32)>,
    /// The index entries of the held parts.
    held_entries: usize,
    /// Parts looked at by `learn`, for the tests.
    #[cfg(test)]
    work: usize,
    seen: BTreeSet<u32>,
    processed: usize,
}

#[derive(Default)]
pub struct Decoder {
    state: Option<State>,
    result: Option<(String, Vec<u8>)>,
}

/// The pieces of a `ur:` text.
pub(crate) struct Parsed {
    pub(crate) ur_type: String,
    /// `(seqNum, seqLen)` from the text of a multi-part part.
    pub(crate) seq: Option<(u32, u32)>,
    pub(crate) bytes: Vec<u8>,
}

fn parse_number(s: &str) -> Option<u32> {
    let canonical = !s.is_empty()
        && s.len() <= 10
        && s.bytes().all(|b| b.is_ascii_digit())
        && (s.len() == 1 || !s.starts_with('0'));
    if canonical {
        s.parse().ok()
    } else {
        None
    }
}

pub(crate) fn parse(text: &str) -> Result<Parsed, UrError> {
    if text.len() > MAX_TEXT_LEN {
        return Err(UrError::TextTooLong);
    }
    let lower = text.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("ur:") else {
        return Err(UrError::Shape("the text does not start with ur:"));
    };
    let mut pieces = rest.split('/');
    let (Some(ur_type), Some(second)) = (pieces.next(), pieces.next()) else {
        return Err(UrError::Shape("a UR is ur:type/body or ur:type/n-m/body"));
    };
    let third = pieces.next();
    if pieces.next().is_some() {
        return Err(UrError::Shape("a UR has at most three parts after ur:"));
    }
    check_type(ur_type)?;
    match third {
        None => Ok(Parsed {
            ur_type: ur_type.to_string(),
            seq: None,
            bytes: decode_minimal(second)?,
        }),
        Some(body) => {
            let seq = second
                .split_once('-')
                .and_then(|(n, m)| Some((parse_number(n)?, parse_number(m)?)));
            let Some(seq) = seq else {
                return Err(UrError::Shape("the sequence is not n-m in plain digits"));
            };
            Ok(Parsed {
                ur_type: ur_type.to_string(),
                seq: Some(seq),
                bytes: decode_minimal(body)?,
            })
        }
    }
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes one part, in upper or lower case. On a refusal the decoder is
    /// as it was.
    pub fn receive(&mut self, text: &str) -> Result<Progress, UrError> {
        let parsed = parse(text)?;
        self.receive_parsed(parsed)
    }

    /// Read a part already parsed by a payload-specific reader, after its
    /// tighter limits have been checked and before allocating stream state.
    pub(crate) fn receive_parsed(&mut self, parsed: Parsed) -> Result<Progress, UrError> {
        if let Some((ur_type, _)) = &self.result {
            if *ur_type != parsed.ur_type {
                return Err(UrError::Disagrees("the type differs from the finished one"));
            }
            return Ok(self.progress());
        }
        let Some((seq_num, seq_len)) = parsed.seq else {
            return self.single(parsed);
        };
        self.multi(parsed.ur_type, seq_num, seq_len, &parsed.bytes)
    }

    /// The type and the CBOR once complete; `None` before.
    pub fn result(&self) -> Option<(String, Vec<u8>)> {
        self.result.clone()
    }

    /// True once the message is complete.
    pub fn is_complete(&self) -> bool {
        self.result.is_some()
    }

    /// Mixed parts held now (for the tests of UR-11).
    #[cfg(test)]
    pub(crate) fn held_mixed(&self) -> usize {
        self.state.as_ref().map_or(0, |s| s.held)
    }

    /// Index entries the held parts have room for, used or not.
    #[cfg(test)]
    pub(crate) fn held_capacity(&self) -> usize {
        self.state.as_ref().map_or(0, |s| {
            s.mixed.iter().flatten().map(|m| m.indexes.capacity()).sum()
        })
    }

    /// Index entries held now, and the parts `learn` has looked at.
    #[cfg(test)]
    pub(crate) fn held_entries_and_work(&self) -> (usize, usize) {
        self.state
            .as_ref()
            .map_or((0, 0), |s| (s.held_entries, s.work))
    }

    fn single(&mut self, parsed: Parsed) -> Result<Progress, UrError> {
        if self.state.is_some() {
            return Err(UrError::Disagrees(
                "a whole UR arrived among multi-part ones",
            ));
        }
        if parsed.bytes.is_empty() || parsed.bytes.len() > MAX_MESSAGE_LEN {
            return Err(UrError::MessageSize);
        }
        self.result = Some((parsed.ur_type, parsed.bytes));
        Ok(self.progress())
    }

    fn multi(
        &mut self,
        ur_type: String,
        text_seq_num: u32,
        text_seq_len: u32,
        body: &[u8],
    ) -> Result<Progress, UrError> {
        let raw = decode_part(body)?;
        let fields = (
            u32::try_from(raw.seq_num),
            u32::try_from(raw.seq_len),
            usize::try_from(raw.message_len),
            u32::try_from(raw.checksum),
        );
        let (Ok(seq_num), Ok(seq_len), Ok(message_len), Ok(checksum)) = fields else {
            return Err(UrError::PartFields(
                "a number is larger than the format allows",
            ));
        };
        if seq_num != text_seq_num || seq_len != text_seq_len {
            return Err(UrError::PartFields(
                "the text and the body disagree on the sequence",
            ));
        }
        let seq_len = seq_len as usize;
        if seq_len == 0 || seq_len > MAX_SEQ_LEN {
            return Err(UrError::SeqLen);
        }
        if seq_num == 0 {
            return Err(UrError::PartFields("the sequence number is 0"));
        }
        if message_len == 0 || message_len > MAX_MESSAGE_LEN {
            return Err(UrError::MessageSize);
        }
        let fragment_len = raw.data.len();
        if !(MIN_FRAGMENT_LEN..=MAX_FRAGMENT_LEN).contains(&fragment_len) {
            return Err(UrError::FragmentLen("the fragment is not 10 to 2000 bytes"));
        }
        // The fragments cover the message and the last one is not empty.
        let covers = seq_len
            .checked_mul(fragment_len)
            .is_some_and(|c| c >= message_len);
        let last_used = (seq_len - 1)
            .checked_mul(fragment_len)
            .is_some_and(|c| c < message_len);
        if !covers || !last_used {
            return Err(UrError::PartFields(
                "the fragment length does not fit the message length and the sequence",
            ));
        }
        match &self.state {
            Some(s) => {
                let why = if s.ur_type != ur_type {
                    Some("the type differs from the first part's")
                } else if s.seq_len != seq_len {
                    Some("the sequence length differs from the first part's")
                } else if s.message_len != message_len {
                    Some("the message length differs from the first part's")
                } else if s.checksum != checksum {
                    Some("the checksum differs from the first part's")
                } else if s.fragment_len != fragment_len {
                    Some("the fragment length differs from the first part's")
                } else {
                    None
                };
                if let Some(why) = why {
                    return Err(UrError::Disagrees(why));
                }
            }
            None => {
                self.state = Some(State {
                    ur_type,
                    seq_len,
                    message_len,
                    checksum,
                    fragment_len,
                    known: vec![None; seq_len],
                    chooser: Chooser::new(seq_len, checksum),
                    mixed: Vec::new(),
                    free: Vec::new(),
                    watch: vec![Vec::new(); seq_len],
                    held: 0,
                    by_left: BTreeSet::new(),
                    held_entries: 0,
                    #[cfg(test)]
                    work: 0,
                    seen: BTreeSet::new(),
                    processed: 0,
                });
            }
        }
        let Some(state) = self.state.as_mut() else {
            return Err(UrError::Disagrees("no state"));
        };
        match state.take_part(seq_num, raw.data) {
            Ok(None) => {}
            Ok(Some(message)) => {
                let ur_type = state.ur_type.clone();
                self.result = Some((ur_type, message));
            }
            Err(e) => {
                // A message that fails its checksum cannot be mended by more
                // parts of the same stream: start again.
                if matches!(e, UrError::Message(_)) {
                    self.state = None;
                }
                return Err(e);
            }
        }
        Ok(self.progress())
    }

    fn progress(&self) -> Progress {
        if self.result.is_some() {
            let expected = self.state.as_ref().map_or(1, |s| s.seq_len);
            let received = self.state.as_ref().map_or(1, |s| s.processed);
            return Progress {
                complete: true,
                received_parts: received.max(1),
                expected_parts: expected,
                estimated_fraction: 1.0,
            };
        }
        match &self.state {
            Some(s) => Progress {
                complete: false,
                received_parts: s.processed,
                expected_parts: s.seq_len,
                estimated_fraction: (s.processed as f64 / (s.seq_len as f64 * 1.75)).min(0.99),
            },
            None => Progress {
                complete: false,
                received_parts: 0,
                expected_parts: 0,
                estimated_fraction: 0.0,
            },
        }
    }
}

impl State {
    /// Takes a part that passed the bounds. Gives the message when this part
    /// completed it.
    fn take_part(&mut self, seq_num: u32, data: &[u8]) -> Result<Option<Vec<u8>>, UrError> {
        if self.seen.contains(&seq_num) {
            return Ok(None);
        }
        // Reduce by what is known, without changing anything yet.
        let mut indexes = self.chooser.choose(seq_num);
        let mut data = data.to_vec();
        indexes.retain(|&i| match self.known.get(i) {
            Some(Some(f)) => {
                xor_into(&mut data, f);
                false
            }
            _ => true,
        });
        let mixed = indexes.len() > 1;
        if mixed && indexes.len() > (self.seq_len / 2).max(MIN_HELD_DEGREE) {
            // Never held, and not counted: the decoder keeps waiting.
            return Ok(None);
        }
        let dup = mixed
            && indexes.first().is_some_and(|&first| {
                self.watch.get(first).is_some_and(|ids| {
                    ids.iter().any(|&id| {
                        self.mixed
                            .get(id as usize)
                            .and_then(Option::as_ref)
                            .is_some_and(|m| m.indexes == indexes)
                    })
                })
            });
        let hold = mixed && !dup && self.make_room(indexes.len());
        if self.seen.len() < 4 * self.seq_len {
            self.seen.insert(seq_num);
        }
        self.processed += 1;
        match indexes.as_slice() {
            [] => Ok(None),
            [one] => self.learn(*one, data),
            _ => {
                if hold {
                    self.hold(indexes, data);
                }
                Ok(None)
            }
        }
    }

    /// Makes room for a mixed part of `len` fragments under the two bounds
    /// of UR-11. A part that does not fit pushes out the held part with the
    /// most fragments still unknown, as long as that has more than the new
    /// one; a new part that is itself the furthest from known is not held.
    /// A sequence of any length read from any part on thus keeps making
    /// progress inside the bounds.
    fn make_room(&mut self, len: usize) -> bool {
        let budget = HELD_ENTRIES_PER_FRAGMENT * self.seq_len;
        while self.held >= 2 * self.seq_len || self.held_entries + len > budget {
            let Some(&(left, id)) = self.by_left.last() else {
                return false;
            };
            if left <= len {
                return false;
            }
            self.by_left.remove(&(left, id));
            let Some(gone) = self.mixed.get_mut(id as usize).and_then(Option::take) else {
                continue;
            };
            self.free.push(id as usize);
            self.held -= 1;
            self.held_entries = self.held_entries.saturating_sub(gone.indexes.len());
            for &k in &gone.indexes {
                if let Some(w) = self.watch.get_mut(k) {
                    w.retain(|&w| w != id);
                }
            }
        }
        true
    }

    fn hold(&mut self, mut indexes: Vec<usize>, data: Vec<u8>) {
        // The list was made at the part's whole degree and cut down by what
        // is known; it keeps only what it holds, so the memory held is what
        // the budget counts (UR-11).
        indexes.shrink_to_fit();
        let id = self.free.pop().unwrap_or(self.mixed.len());
        for &i in &indexes {
            if let Some(w) = self.watch.get_mut(i) {
                w.push(id as u32);
            }
        }
        self.held += 1;
        self.held_entries += indexes.len();
        self.by_left.insert((indexes.len(), id as u32));
        let part = Some(Mixed {
            left: indexes.len(),
            indexes,
            data,
        });
        match self.mixed.get_mut(id) {
            Some(slot) => *slot = part,
            None => self.mixed.push(part),
        }
    }

    /// A fragment becomes known; the held parts that carry it shrink, and
    /// any that reach one fragment become known in turn. Each entry of a
    /// held part is looked at once, when its fragment is learned.
    fn learn(&mut self, index: usize, data: Vec<u8>) -> Result<Option<Vec<u8>>, UrError> {
        let mut work = vec![(index, data)];
        while let Some((i, d)) = work.pop() {
            match self.known.get(i) {
                Some(None) => {}
                _ => continue,
            }
            let ids = self
                .watch
                .get_mut(i)
                .map(std::mem::take)
                .unwrap_or_default();
            if let Some(slot) = self.known.get_mut(i) {
                *slot = Some(d.clone());
            }
            for id in ids {
                #[cfg(test)]
                {
                    self.work += 1;
                }
                let Some(m) = self.mixed.get_mut(id as usize).and_then(Option::as_mut) else {
                    continue;
                };
                xor_into(&mut m.data, &d);
                self.by_left.remove(&(m.left, id));
                m.left = m.left.saturating_sub(1);
                if m.left > 1 {
                    self.by_left.insert((m.left, id));
                    continue;
                }
                let Some(done) = self.mixed.get_mut(id as usize).and_then(Option::take) else {
                    continue;
                };
                self.free.push(id as usize);
                self.held -= 1;
                self.held_entries = self.held_entries.saturating_sub(done.indexes.len());
                let known = &self.known;
                let rest = done
                    .indexes
                    .iter()
                    .copied()
                    .find(|&k| matches!(known.get(k), Some(None)));
                if let Some(one) = rest {
                    // The slot may be reused: no stale entry may stay.
                    if let Some(w) = self.watch.get_mut(one) {
                        w.retain(|&w| w != id);
                    }
                    work.push((one, done.data));
                }
            }
        }
        if self.known.iter().any(Option::is_none) {
            return Ok(None);
        }
        self.join().map(Some)
    }

    /// The message from all the fragments, checked (UR-12).
    fn join(&self) -> Result<Vec<u8>, UrError> {
        let mut message: Vec<u8> = self.known.iter().flatten().flatten().copied().collect();
        if message.len() < self.message_len {
            return Err(UrError::Message(
                "the fragments are shorter than the message",
            ));
        }
        let padding = message.split_off(self.message_len);
        if padding.iter().any(|&b| b != 0) {
            return Err(UrError::Message("the padding is not zeros"));
        }
        if crc32(&message) != self.checksum {
            return Err(UrError::Message("the message does not match its checksum"));
        }
        Ok(message)
    }
}

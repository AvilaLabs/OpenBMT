//! SK-25: the check of the written words. Three positions among the 12 are
//! asked back, picked from the kernel's random bytes, and the owner types the
//! first four letters of each word (as KM-16).

/// Positions asked.
pub const ASKED: usize = 3;
/// Letters typed of each word.
pub const LETTERS: usize = 4;

/// Three distinct positions (0 to 11, ascending): the bytes in order, each
/// below 252 taken modulo 12, skipping repeats; the lowest free places if
/// the bytes run out.
pub fn pick_positions(bytes: &[u8]) -> [usize; ASKED] {
    let mut out: Vec<usize> = Vec::with_capacity(ASKED);
    for b in bytes {
        if out.len() == ASKED {
            break;
        }
        if *b >= 252 {
            continue;
        }
        let p = usize::from(*b) % 12;
        if !out.contains(&p) {
            out.push(p);
        }
    }
    let mut p = 0;
    while out.len() < ASKED {
        if !out.contains(&p) {
            out.push(p);
        }
        p += 1;
    }
    out.sort_unstable();
    [out[0], out[1], out[2]]
}

/// Whether the typed letters are the first four letters of `word` (the whole
/// word if shorter), in any letter case.
pub fn answer_matches(word: &str, typed: &str) -> bool {
    let n = word.len().min(LETTERS);
    let t = typed.to_ascii_lowercase();
    t.len() >= n && t.get(..n) == word.get(..n)
}

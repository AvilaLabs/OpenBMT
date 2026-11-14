//! Seed words from an owner's own dice rolls or coin flips.
//!
//! There are two methods. Both are pure functions: they take the throws and
//! return words. This module has no random source and does no I/O.
//!
//! **Dice, hashed.** The owner rolls a six-sided die at least 99 times and
//! writes down each face as a digit `1` to `6`. The words are the 24 BIP39
//! words for the SHA-256 hash of those digits as ASCII text. Nothing else
//! goes into the hash. It leans on the hash to even out a die that is not
//! quite fair.
//!
//! **Skew-corrected (von Neumann).** Throws are taken in pairs. For a coin,
//! heads then tails is a 1 bit, tails then heads is a 0 bit, and two the
//! same are thrown away. For a die, a second roll higher than the first is
//! a 1 bit, lower is a 0 bit, and two the same are thrown away. The first
//! 128 or 256 bits, most significant bit of each byte first, are the BIP39
//! entropy for 12 or 24 words. No hash is used.
//!
//! Nothing is mixed in with either method. The same throws give the same
//! words in any tool that follows the same steps, so the owner can check
//! the result elsewhere or work it by hand.
//!
//! Limits. Von Neumann's method needs throws that do not affect each other,
//! and the same coin or die thrown the same way from start to finish. It
//! cannot fix throws that a person made up. The hashed method leans on the
//! hash: it does not make a bad source good, it only hides a small lean.

use std::fmt;

use bitcoin::hashes::{sha256, Hash};

/// Fewest rolls the hashed method accepts (99 rolls of a six-sided die is
/// about 256 bits).
pub const MIN_HASHED_ROLLS: usize = 99;
/// Most rolls the hashed method accepts.
pub const MAX_HASHED_ROLLS: usize = 10_000;
/// Most dice rolls `bits_from_dice` accepts in one call.
pub const MAX_DICE_ROLLS: usize = 100_000;

/// Why throws could not be turned into words or bits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntropyError {
    /// A character that is not a die face `1` to `6`. `position` counts
    /// bytes from 0.
    NotADieFace { position: usize },
    /// Fewer rolls than the hashed method needs.
    TooFewRolls { have: usize, need: usize },
    /// More throws than one call takes.
    TooManyThrows { have: usize, max: usize },
    /// Fewer bits than the word count needs.
    NeedMoreBits { have: usize, need: usize },
    /// The entropy was refused when making words. This cannot happen for
    /// the lengths this module builds.
    Words,
}

impl fmt::Display for EntropyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotADieFace { position } => write!(
                f,
                "character {} is not a die face (use only 1 to 6)",
                position.saturating_add(1)
            ),
            Self::TooFewRolls { have, need } => {
                write!(f, "{have} rolls is too few, at least {need} are needed")
            }
            Self::TooManyThrows { have, max } => {
                write!(f, "{have} throws is too many, the most allowed is {max}")
            }
            Self::NeedMoreBits { have, need } => {
                write!(f, "{have} bits is too few, {need} are needed")
            }
            Self::Words => write!(f, "the words could not be made"),
        }
    }
}

impl std::error::Error for EntropyError {}

/// Seed words made from the owner's throws. The words are a secret: `Debug`
/// hides them and there is no `Display`.
pub struct SeedWords(bip39::Mnemonic);

impl SeedWords {
    fn from_entropy(entropy: &[u8]) -> Result<Self, EntropyError> {
        bip39::Mnemonic::from_entropy(entropy)
            .map(Self)
            .map_err(|_| EntropyError::Words)
    }

    /// The words, in order, from the English list.
    pub fn words(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.0.words()
    }

    /// 12 or 24.
    pub fn word_count(&self) -> usize {
        self.0.word_count()
    }

    /// The BIP39 entropy behind the words (16 or 32 bytes).
    pub fn to_entropy(&self) -> Vec<u8> {
        self.0.to_entropy()
    }

    /// The same words as a `bip39::Mnemonic`.
    pub fn into_mnemonic(self) -> bip39::Mnemonic {
        self.0
    }
}

impl fmt::Debug for SeedWords {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SeedWords(<hidden>)")
    }
}

/// Overwrite the words of a mnemonic in place with the words of all-zero
/// entropy of the same length. The `bip39` crate is built without its
/// `zeroize` feature (it would add a crate that is not in the lock file), so
/// the words are replaced by a value of the same type and the write is kept
/// from being dropped as a dead store.
pub fn wipe_mnemonic(m: &mut bip39::Mnemonic) {
    let zeros = [0u8; 32];
    let bytes = m.word_count() / 3 * 4;
    if let Some(blank) = zeros
        .get(..bytes)
        .and_then(|e| bip39::Mnemonic::from_entropy(e).ok())
    {
        *m = blank;
    }
    std::hint::black_box(&mut *m);
}

/// Overwrite the bytes of a text that held words or a seed, and empty it.
pub fn wipe_string(s: &mut String) {
    let mut bytes = std::mem::take(s).into_bytes();
    bytes.fill(0);
    std::hint::black_box(&mut bytes);
}

/// One coin flip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flip {
    Heads,
    Tails,
}

/// How many words to make from skew-corrected bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordCount {
    Twelve,
    TwentyFour,
}

impl WordCount {
    /// Bits of entropy needed: 128 or 256.
    pub fn bits(self) -> usize {
        match self {
            Self::Twelve => 128,
            Self::TwentyFour => 256,
        }
    }
}

/// Check the digits and return each face as 1 to 6.
fn parse_faces(rolls: &str, max: usize) -> Result<Vec<u8>, EntropyError> {
    let have = rolls.len();
    if have > max {
        return Err(EntropyError::TooManyThrows { have, max });
    }
    rolls
        .bytes()
        .enumerate()
        .map(|(position, b)| match b {
            b'1'..=b'6' => Ok(b - b'0'),
            _ => Err(EntropyError::NotADieFace { position }),
        })
        .collect()
}

/// Dice, hashed: 24 words from the SHA-256 hash of the roll digits.
///
/// `rolls` holds only the ASCII digits `1` to `6`, in the order rolled, at
/// least 99 of them. All of them are hashed, exactly as given.
pub fn words_from_dice_hashed(rolls: &str) -> Result<SeedWords, EntropyError> {
    let faces = parse_faces(rolls, MAX_HASHED_ROLLS)?;
    if faces.len() < MIN_HASHED_ROLLS {
        return Err(EntropyError::TooFewRolls {
            have: faces.len(),
            need: MIN_HASHED_ROLLS,
        });
    }
    let hash = sha256::Hash::hash(rolls.as_bytes());
    SeedWords::from_entropy(hash.as_byte_array())
}

/// How often each face came up: index 0 counts ones, index 5 counts sixes.
pub fn face_counts(rolls: &str) -> Result<[u32; 6], EntropyError> {
    let faces = parse_faces(rolls, MAX_HASHED_ROLLS)?;
    let mut counts = [0u32; 6];
    for face in faces {
        let slot = usize::from(face).saturating_sub(1);
        if let Some(c) = counts.get_mut(slot) {
            *c = c.saturating_add(1);
        }
    }
    Ok(counts)
}

/// Skew-corrected bits from coin flips, taken in pairs. Heads then tails is
/// 1, tails then heads is 0, two the same give nothing. A last flip without
/// a partner is ignored.
pub fn bits_from_coin(flips: &[Flip]) -> Vec<bool> {
    flips
        .as_chunks::<2>()
        .0
        .iter()
        .filter_map(|pair| match pair {
            [Flip::Heads, Flip::Tails] => Some(true),
            [Flip::Tails, Flip::Heads] => Some(false),
            _ => None,
        })
        .collect()
}

/// Skew-corrected bits from die rolls, taken in pairs. A second roll higher
/// than the first is 1, lower is 0, equal gives nothing. A last roll without
/// a partner is ignored.
pub fn bits_from_dice(rolls: &str) -> Result<Vec<bool>, EntropyError> {
    let faces = parse_faces(rolls, MAX_DICE_ROLLS)?;
    Ok(faces
        .as_chunks::<2>()
        .0
        .iter()
        .filter_map(|pair| match pair {
            [first, second] if second > first => Some(true),
            [first, second] if second < first => Some(false),
            _ => None,
        })
        .collect())
}

/// 12 or 24 words from skew-corrected bits. Uses exactly the first 128 or
/// 256 bits, most significant bit of each byte first; later bits are
/// ignored.
pub fn words_from_bits(bits: &[bool], count: WordCount) -> Result<SeedWords, EntropyError> {
    let need = count.bits();
    if bits.len() < need {
        return Err(EntropyError::NeedMoreBits {
            have: bits.len(),
            need,
        });
    }
    let used: Vec<bool> = bits.iter().copied().take(need).collect();
    let entropy: Vec<u8> = used
        .chunks(8)
        .map(|byte| {
            byte.iter()
                .fold(0u8, |acc, bit| (acc << 1) | u8::from(*bit))
        })
        .collect();
    SeedWords::from_entropy(&entropy)
}

/// How many more bits are needed for `count` words, for a progress display.
/// Zero once there are enough.
pub fn bits_still_needed(bits: usize, count: WordCount) -> usize {
    count.bits().saturating_sub(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wiped_mnemonic_holds_the_words_of_zero_entropy_and_a_wiped_text_is_empty() {
        for bytes in [16usize, 32] {
            let mut m = bip39::Mnemonic::from_entropy(&vec![0x5a; bytes]).unwrap();
            wipe_mnemonic(&mut m);
            assert_eq!(m.to_entropy(), vec![0u8; bytes]);
            assert_eq!(m.word_count(), bytes / 4 * 3);
        }
        let mut text = "abandon ability".to_string();
        wipe_string(&mut text);
        assert!(text.is_empty());
    }

    fn words_of(w: &SeedWords) -> String {
        w.words().collect::<Vec<_>>().join(" ")
    }

    fn repeat_words(word: &str, n: usize, last: &str) -> String {
        let mut v = vec![word; n];
        v.push(last);
        v.join(" ")
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// 99 rolls; the hash and the words were worked out outside this code.
    const ROLLS_99: &str = "155155155155155155155155155155155155155155155155155155155155155155155155155155155155155155155155155";
    const ROLLS_99_HASH: &str = "8d664195cf3896cd3f8ccf0dbe268ad137c3196be49e85c63f0c4518376e36e4";
    const ROLLS_99_WORDS: &str = "miracle craft grace oyster maximum grief wreck okay asset van pencil pen label million question need magic cradle seek mechanic lock require swear mutual";
    const ROLLS_102_HASH: &str = "8fd128918b2e29d6dcbfa5b9a118e5c16d60498c7ba107922a8eb6eb1d36c112";

    /// A fixed linear congruential generator (Knuth's MMIX constants).
    struct Lcg(u64);

    impl Lcg {
        /// A number from 0 to 999.
        fn next_thousand(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) % 1000
        }
    }

    fn ones_share(bits: &[bool]) -> f64 {
        let ones = bits.iter().filter(|b| **b).count();
        ones as f64 / bits.len() as f64
    }

    #[test]
    fn all_zero_bits_give_the_bip39_vector_for_twelve_words() {
        let w = words_from_bits(&[false; 128], WordCount::Twelve).unwrap();
        assert_eq!(words_of(&w), repeat_words("abandon", 11, "about"));
        assert_eq!(w.word_count(), 12);
    }

    #[test]
    fn all_zero_bits_give_the_bip39_vector_for_twenty_four_words() {
        let w = words_from_bits(&[false; 256], WordCount::TwentyFour).unwrap();
        assert_eq!(words_of(&w), repeat_words("abandon", 23, "art"));
        assert_eq!(w.word_count(), 24);
    }

    #[test]
    fn all_one_bits_give_the_bip39_vector_for_twelve_words() {
        let w = words_from_bits(&[true; 128], WordCount::Twelve).unwrap();
        assert_eq!(words_of(&w), repeat_words("zoo", 11, "wrong"));
    }

    #[test]
    fn first_bit_is_the_most_significant_bit_of_the_first_byte() {
        let mut bits = vec![false; 128];
        bits[0] = true;
        let w = words_from_bits(&bits, WordCount::Twelve).unwrap();
        let entropy = w.to_entropy();
        assert_eq!(entropy.len(), 16);
        assert_eq!(entropy[0], 0x80);
        assert!(entropy[1..].iter().all(|b| *b == 0));
        // The eighth bit of a byte is the least significant.
        let mut bits = vec![false; 128];
        bits[7] = true;
        let w = words_from_bits(&bits, WordCount::Twelve).unwrap();
        assert_eq!(w.to_entropy()[0], 0x01);
    }

    #[test]
    fn only_the_first_bits_are_used_and_later_bits_are_ignored() {
        let mut bits = vec![false; 300];
        for b in bits.iter_mut().skip(256) {
            *b = true;
        }
        let w = words_from_bits(&bits, WordCount::TwentyFour).unwrap();
        assert_eq!(words_of(&w), repeat_words("abandon", 23, "art"));
        let twelve = words_from_bits(&bits, WordCount::Twelve).unwrap();
        assert_eq!(twelve.to_entropy(), vec![0u8; 16]);
    }

    #[test]
    fn too_few_bits_are_refused_at_127_and_255() {
        assert_eq!(
            words_from_bits(&[false; 127], WordCount::Twelve).unwrap_err(),
            EntropyError::NeedMoreBits {
                have: 127,
                need: 128
            }
        );
        assert_eq!(
            words_from_bits(&[false; 255], WordCount::TwentyFour).unwrap_err(),
            EntropyError::NeedMoreBits {
                have: 255,
                need: 256
            }
        );
        assert!(words_from_bits(&[false; 128], WordCount::Twelve).is_ok());
        assert!(words_from_bits(&[false; 256], WordCount::TwentyFour).is_ok());
    }

    #[test]
    fn bits_still_needed_counts_down_and_stops_at_zero() {
        assert_eq!(bits_still_needed(0, WordCount::Twelve), 128);
        assert_eq!(bits_still_needed(100, WordCount::Twelve), 28);
        assert_eq!(bits_still_needed(128, WordCount::Twelve), 0);
        assert_eq!(bits_still_needed(500, WordCount::Twelve), 0);
        assert_eq!(bits_still_needed(0, WordCount::TwentyFour), 256);
        assert_eq!(bits_still_needed(255, WordCount::TwentyFour), 1);
        assert_eq!(bits_still_needed(256, WordCount::TwentyFour), 0);
    }

    #[test]
    fn coin_pairs_give_bits_and_equal_pairs_give_nothing() {
        use Flip::{Heads as H, Tails as T};
        assert_eq!(bits_from_coin(&[H, T]), vec![true]);
        assert_eq!(bits_from_coin(&[T, H]), vec![false]);
        assert!(bits_from_coin(&[H, H]).is_empty());
        assert!(bits_from_coin(&[T, T]).is_empty());
        assert!(bits_from_coin(&[]).is_empty());
    }

    #[test]
    fn coin_keeps_order_and_ignores_an_odd_last_flip() {
        use Flip::{Heads as H, Tails as T};
        // HT, HH, TH, TT, TH, then a lone H with no partner.
        let flips = [H, T, H, H, T, H, T, T, T, H, H];
        assert_eq!(bits_from_coin(&flips), vec![true, false, false]);
        assert!(bits_from_coin(&[H]).is_empty());
    }

    #[test]
    fn two_hundred_fifty_six_tails_heads_pairs_give_the_all_zero_twenty_four_words() {
        let flips: Vec<Flip> = (0..256).flat_map(|_| [Flip::Tails, Flip::Heads]).collect();
        let bits = bits_from_coin(&flips);
        assert_eq!(bits.len(), 256);
        let w = words_from_bits(&bits, WordCount::TwentyFour).unwrap();
        assert_eq!(words_of(&w), repeat_words("abandon", 23, "art"));
    }

    #[test]
    fn every_unequal_pair_of_faces_gives_the_bit_for_which_is_higher() {
        let mut checked = 0;
        for a in 1..=6u8 {
            for b in 1..=6u8 {
                let rolls = format!("{a}{b}");
                let bits = bits_from_dice(&rolls).unwrap();
                if a == b {
                    assert!(bits.is_empty(), "{rolls}");
                } else {
                    assert_eq!(bits, vec![b > a], "{rolls}");
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 30);
    }

    #[test]
    fn dice_bits_keep_order_and_ignore_an_odd_last_roll() {
        // 12 up, 21 down, 33 equal, then a lone 6.
        assert_eq!(bits_from_dice("1221336").unwrap(), vec![true, false]);
        assert!(bits_from_dice("4").unwrap().is_empty());
        assert!(bits_from_dice("").unwrap().is_empty());
    }

    #[test]
    fn dice_reject_anything_but_faces_one_to_six_and_say_where() {
        for (text, position) in [
            ("12a4", 2),
            ("0123", 0),
            ("1237", 3),
            ("12 3", 2),
            ("12\n3", 2),
            ("123456x", 6),
        ] {
            assert_eq!(
                bits_from_dice(text).unwrap_err(),
                EntropyError::NotADieFace { position },
                "{text:?}"
            );
            assert_eq!(
                face_counts(text).unwrap_err(),
                EntropyError::NotADieFace { position },
                "{text:?}"
            );
        }
        let mut long = "1".repeat(99);
        long.push(' ');
        assert_eq!(
            words_from_dice_hashed(&long).unwrap_err(),
            EntropyError::NotADieFace { position: 99 }
        );
    }

    #[test]
    fn a_non_ascii_character_is_refused() {
        assert!(matches!(
            bits_from_dice("1\u{e9}2"),
            Err(EntropyError::NotADieFace { position: 1 })
        ));
    }

    #[test]
    fn hashed_dice_refuse_98_accept_99_and_100() {
        let ninety_eight = &ROLLS_99[..98];
        assert_eq!(
            words_from_dice_hashed(ninety_eight).unwrap_err(),
            EntropyError::TooFewRolls { have: 98, need: 99 }
        );
        assert_eq!(
            words_from_dice_hashed("").unwrap_err(),
            EntropyError::TooFewRolls { have: 0, need: 99 }
        );
        let first = words_from_dice_hashed(ROLLS_99).unwrap();
        let hundred = format!("{ROLLS_99}3");
        let second = words_from_dice_hashed(&hundred).unwrap();
        assert_eq!(second.word_count(), 24);
        assert_ne!(first.to_entropy(), second.to_entropy());
    }

    #[test]
    fn hashed_dice_have_a_cap() {
        let at_cap = "1".repeat(MAX_HASHED_ROLLS);
        assert!(words_from_dice_hashed(&at_cap).is_ok());
        let over = "1".repeat(MAX_HASHED_ROLLS + 1);
        assert_eq!(
            words_from_dice_hashed(&over).unwrap_err(),
            EntropyError::TooManyThrows {
                have: MAX_HASHED_ROLLS + 1,
                max: MAX_HASHED_ROLLS
            }
        );
    }

    #[test]
    fn dice_bits_have_a_cap() {
        let at_cap = "12".repeat(MAX_DICE_ROLLS / 2);
        assert_eq!(bits_from_dice(&at_cap).unwrap().len(), MAX_DICE_ROLLS / 2);
        let over = "1".repeat(MAX_DICE_ROLLS + 1);
        assert_eq!(
            bits_from_dice(&over).unwrap_err(),
            EntropyError::TooManyThrows {
                have: MAX_DICE_ROLLS + 1,
                max: MAX_DICE_ROLLS
            }
        );
    }

    #[test]
    fn hashed_dice_entropy_is_the_sha256_of_the_digits_checked_outside_rust() {
        // Hashes from `python3 -I -c "import hashlib; ..."`; words from a
        // by-hand BIP39 script reading the crate's English list.
        let w = words_from_dice_hashed(ROLLS_99).unwrap();
        assert_eq!(w.word_count(), 24);
        assert_eq!(hex(&w.to_entropy()), ROLLS_99_HASH);
        assert_eq!(words_of(&w), ROLLS_99_WORDS);

        let rolls = "123456".repeat(17);
        assert_eq!(rolls.len(), 102);
        let w = words_from_dice_hashed(&rolls).unwrap();
        assert_eq!(hex(&w.to_entropy()), ROLLS_102_HASH);
    }

    #[test]
    fn face_counts_are_right_on_a_mixed_string() {
        assert_eq!(face_counts("").unwrap(), [0; 6]);
        assert_eq!(face_counts("1122223666666").unwrap(), [2, 4, 1, 0, 0, 6]);
        assert_eq!(face_counts("5").unwrap(), [0, 0, 0, 0, 1, 0]);
    }

    #[test]
    fn skew_correction_evens_out_a_coin_that_lands_heads_80_percent() {
        let mut rng = Lcg(12345);
        let flips: Vec<Flip> = (0..200_000)
            .map(|_| {
                if rng.next_thousand() < 800 {
                    Flip::Heads
                } else {
                    Flip::Tails
                }
            })
            .collect();
        let heads = flips.iter().filter(|f| **f == Flip::Heads).count();
        let raw = heads as f64 / flips.len() as f64;
        assert!(!(0.48..=0.52).contains(&raw), "raw source share {raw}");
        let bits = bits_from_coin(&flips);
        assert!(bits.len() > 10_000);
        let share = ones_share(&bits);
        assert!((0.48..=0.52).contains(&share), "corrected share {share}");
    }

    #[test]
    fn skew_correction_evens_out_a_die_that_shows_six_half_the_time() {
        let mut rng = Lcg(98765);
        let faces: Vec<u8> = (0..200_000)
            .map(|_| {
                if rng.next_thousand() < 500 {
                    6
                } else {
                    // Faces 1 to 5, evenly.
                    u8::try_from(rng.next_thousand() % 5).unwrap_or(0) + 1
                }
            })
            .collect();
        let rolls: String = faces.iter().map(|f| char::from(b'0' + f)).collect();
        let sixes = faces.iter().filter(|f| **f == 6).count();
        let six_share = sixes as f64 / faces.len() as f64;
        // A fair die shows 6 one time in six; this one shows it half the time.
        assert!((0.48..=0.52).contains(&six_share), "sixes {six_share}");
        // Its share of high rolls (4 to 6) is about 70%, far from half.
        let high = faces.iter().filter(|f| **f > 3).count() as f64 / faces.len() as f64;
        assert!(!(0.48..=0.52).contains(&high), "raw source share {high}");
        // One call takes at most 100,000 rolls, so go in two halves (each an
        // even length, so no pair is split).
        let (first, second) = rolls.split_at(rolls.len() / 2);
        let mut bits = bits_from_dice(first).unwrap();
        bits.extend(bits_from_dice(second).unwrap());
        assert!(bits.len() > 10_000);
        let share = ones_share(&bits);
        assert!((0.48..=0.52).contains(&share), "corrected share {share}");
    }

    #[test]
    fn debug_of_seed_words_hides_every_word() {
        let w = words_from_bits(&[true; 128], WordCount::Twelve).unwrap();
        let shown = format!("{w:?}");
        assert_eq!(shown, "SeedWords(<hidden>)");
        assert!(!shown.contains("zoo"));
        assert!(!shown.contains("wrong"));
    }

    #[test]
    fn into_mnemonic_keeps_the_words() {
        let w = words_from_bits(&[false; 128], WordCount::Twelve).unwrap();
        let m = w.into_mnemonic();
        assert_eq!(m.to_string(), repeat_words("abandon", 11, "about"));
    }

    #[test]
    fn errors_have_readable_messages() {
        let e = EntropyError::NotADieFace { position: 4 };
        assert_eq!(
            e.to_string(),
            "character 5 is not a die face (use only 1 to 6)"
        );
        assert!(EntropyError::TooFewRolls { have: 3, need: 99 }
            .to_string()
            .contains("99"));
    }
}

//! SS-10, SS-11: the words shown once, four at a time, as an image the Rust
//! core draws; then the check-back of three positions, answered on a keypad
//! whose letters come straight here. The app gets back an image, a count of
//! letters and a yes, never a word as text.

use std::fmt;

use bip39::Language;
use openbmt_core::font::{FIRST, GLYPHS, GLYPH_H, GLYPH_W, LAST};

use super::chance::{checked_read, make_seed, KernelRandom, Seed, StarterWallet};
use super::seal::{seal_seed, Sealed};
use super::{wipe, StarterRefusal};

/// SS-10: words on one screen.
pub const WORDS_PER_PAGE: usize = 4;
/// SS-10: screens of words.
pub const PAGES: usize = 3;
/// SS-10: positions asked back.
pub const ASKED: usize = 3;
/// Letters of a word that the check-back takes (KM-16).
const LETTERS: usize = 4;
/// The longest word of the list.
const MAX_LETTERS: usize = 8;
/// The most a screen is scaled.
const MAX_SCALE: u32 = 16;

/// SS-11: a grey image the Rust core drew, one byte for each pixel, row by
/// row, 0 black ink on 255 paper. It is wiped when dropped.
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height` bytes.
    pub pixels: Vec<u8>,
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Image({}x{}, <hidden>)", self.width, self.height)
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        wipe(&mut self.pixels);
    }
}

/// Draws `lines` of text, left aligned, in the 10 x 20 font at `scale`.
fn draw(lines: &[String], scale: u32) -> Image {
    let scale = scale.clamp(1, MAX_SCALE) as usize;
    let cols = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let margin = GLYPH_W * scale;
    let pitch = (GLYPH_H + 8) * scale;
    let width = margin * 2 + cols * GLYPH_W * scale;
    let height = margin * 2 + lines.len() * pitch;
    let mut pixels = vec![255u8; width * height];
    for (line_no, line) in lines.iter().enumerate() {
        for (col_no, ch) in line.chars().enumerate() {
            let byte = u8::try_from(ch)
                .ok()
                .filter(|b| (FIRST..=LAST).contains(b))
                .unwrap_or(b'?');
            let Some(rows) = GLYPHS.get(usize::from(byte - FIRST)) else {
                continue;
            };
            for (row, bits) in rows.iter().enumerate() {
                for bit in 0..GLYPH_W {
                    if bits >> (GLYPH_W - 1 - bit) & 1 == 0 {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = margin + (col_no * GLYPH_W + bit) * scale + dx;
                            let y = margin + line_no * pitch + row * scale + dy;
                            if let Some(p) = pixels.get_mut(y * width + x) {
                                *p = 0;
                            }
                        }
                    }
                }
            }
        }
    }
    Image {
        width: u32::try_from(width).unwrap_or(u32::MAX),
        height: u32::try_from(height).unwrap_or(u32::MAX),
        pixels,
    }
}

/// A key of the keypad (SS-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A letter, a to z in either case.
    Letter(u8),
    /// Takes back the last letter (and, in the restore, the last word when
    /// no letter is typed).
    Backspace,
    /// Takes the word typed, when it is accepted.
    Enter,
}

impl Key {
    /// The key of a byte as the app sends it: a letter, 8 for backspace,
    /// 10 or 13 for enter.
    pub fn from_byte(b: u8) -> Result<Self, StarterRefusal> {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' => Ok(Self::Letter(b.to_ascii_lowercase())),
            8 => Ok(Self::Backspace),
            10 | 13 => Ok(Self::Enter),
            _ => Err(StarterRefusal::Key),
        }
    }
}

/// What a key did: no letters, only counts and yes or no (SS-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Typed {
    /// Letters typed of the word now being answered.
    pub typed: usize,
    /// Whether those letters are accepted as the word asked (in the check)
    /// or as one word of the list (in the restore).
    pub accepted: bool,
    /// Words taken so far (answers in the check, words in the restore).
    pub step: usize,
    /// All the words were taken.
    pub finished: bool,
    /// The check was answered wrong: the words are to be shown again.
    pub failed: bool,
}

/// The letters being typed, wiped when dropped.
#[derive(Default)]
pub(crate) struct Pad(Vec<u8>);

impl Pad {
    pub(crate) fn push(&mut self, letter: u8) {
        if self.0.len() < MAX_LETTERS {
            self.0.push(letter);
        }
    }

    pub(crate) fn pop(&mut self) -> bool {
        self.0.pop().is_some()
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn clear(&mut self) {
        wipe(&mut self.0);
        self.0.clear();
    }

    /// The word of the list the letters mean: a whole word, or the one word
    /// that starts with them (the first four letters of a word are unique).
    pub(crate) fn complete(&self) -> Option<&'static str> {
        if self.0.is_empty() {
            return None;
        }
        let list = Language::English.word_list();
        if let Some(w) = list.iter().find(|w| w.as_bytes() == self.0.as_slice()) {
            return Some(w);
        }
        let mut hits = list.iter().filter(|w| w.as_bytes().starts_with(&self.0));
        match (hits.next(), hits.next()) {
            (Some(w), None) => Some(w),
            _ => None,
        }
    }

    /// KM-16: the letters are the first four of `word` (all of it when it
    /// is shorter).
    pub(crate) fn answers(&self, word: &str) -> bool {
        let n = word.len().min(LETTERS);
        self.0.len() >= n && word.as_bytes().get(..n) == self.0.get(..n)
    }
}

impl Drop for Pad {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// SS-10: the three positions, 0 to 11 ascending, from the bytes by KM-16's rule:
/// the bytes in order, each below 252 taken modulo 12, skipping repeats; the
/// lowest free places if the bytes run out.
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
    let mut it = out.into_iter();
    [
        it.next().unwrap_or(0),
        it.next().unwrap_or(1),
        it.next().unwrap_or(2),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    /// Made; the words may be shown.
    Made,
    /// A check is being answered.
    Checking,
    /// A check was answered wrong; the words must be shown again.
    Failed { reshown: bool },
    /// The check was passed; the words are not shown again.
    Passed,
}

/// The making of the starter wallet, from the words made to the seal
/// (SS-10, SS-11). The caller holds it between calls; it holds the seed and
/// wipes it when dropped.
pub struct Setup {
    pub(super) seed: Seed,
    wallet: StarterWallet,
    pub(super) stage: Stage,
    positions: [usize; ASKED],
    answered: usize,
    pad: Pad,
}

impl fmt::Debug for Setup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Setup({:?})", self.stage)
    }
}

impl Setup {
    /// SS-01 to SS-04: make the words from the kernel's bytes and the rolls.
    pub fn begin(rng: &mut dyn KernelRandom, rolls: &[u8]) -> Result<Self, StarterRefusal> {
        let seed = make_seed(rng, rolls)?;
        let wallet = seed.wallet()?;
        Ok(Self {
            seed,
            wallet,
            stage: Stage::Made,
            positions: [0, 1, 2],
            answered: 0,
            pad: Pad::default(),
        })
    }

    /// The public side of the wallet.
    pub fn wallet(&self) -> &StarterWallet {
        &self.wallet
    }

    /// SS-10, SS-11: screen `page` (0 to 2) of four numbered words, drawn
    /// at `scale`. Only before the check is passed, and not while it is
    /// being answered; after a wrong answer, the screens are shown again.
    pub fn page(&mut self, page: usize, scale: u32) -> Result<Image, StarterRefusal> {
        match self.stage {
            Stage::Made => {}
            Stage::Failed { .. } => self.stage = Stage::Failed { reshown: true },
            Stage::Checking | Stage::Passed => return Err(StarterRefusal::Stage),
        }
        if page >= PAGES {
            return Err(StarterRefusal::Page);
        }
        let mnemonic = self.seed.mnemonic()?;
        let lines: Vec<String> = mnemonic
            .words()
            .enumerate()
            .skip(page * WORDS_PER_PAGE)
            .take(WORDS_PER_PAGE)
            .map(|(i, w)| format!("{:>2} {w}", i + 1))
            .collect();
        Ok(draw(&lines, scale))
    }

    /// SS-10: pick three positions from fresh kernel bytes and begin the
    /// check. Returned 1 to 12, ascending: they are not secret. After a
    /// wrong answer the words must have been shown again first.
    pub fn start_check(
        &mut self,
        rng: &mut dyn KernelRandom,
    ) -> Result<[usize; ASKED], StarterRefusal> {
        match self.stage {
            Stage::Made | Stage::Checking => {}
            Stage::Failed { reshown: true } => {}
            Stage::Failed { reshown: false } | Stage::Passed => return Err(StarterRefusal::Stage),
        }
        let mut bytes = checked_read(rng)?;
        self.positions = pick_positions(&bytes);
        wipe(&mut bytes);
        self.stage = Stage::Checking;
        self.answered = 0;
        self.pad.clear();
        Ok(self.positions.map(|p| p + 1))
    }

    /// SS-11: a key of the keypad. Only while a check is being answered.
    pub fn key(&mut self, key: Key) -> Result<Typed, StarterRefusal> {
        if self.stage != Stage::Checking {
            return Err(StarterRefusal::Stage);
        }
        match key {
            Key::Letter(l) => self.pad.push(l),
            Key::Backspace => {
                self.pad.pop();
            }
            Key::Enter => {
                if self.accepted()? {
                    self.pad.clear();
                    self.answered += 1;
                    if self.answered == ASKED {
                        self.stage = Stage::Passed;
                    }
                } else {
                    self.pad.clear();
                    self.stage = Stage::Failed { reshown: false };
                }
            }
        }
        Ok(Typed {
            typed: self.pad.len(),
            accepted: self.stage == Stage::Checking && self.accepted()?,
            step: self.answered,
            finished: self.stage == Stage::Passed,
            failed: matches!(self.stage, Stage::Failed { .. }),
        })
    }

    fn accepted(&self) -> Result<bool, StarterRefusal> {
        let Some(position) = self.positions.get(self.answered) else {
            return Ok(false);
        };
        let mnemonic = self.seed.mnemonic()?;
        let answered = mnemonic
            .words()
            .nth(*position)
            .is_some_and(|w| self.pad.answers(w));
        Ok(answered)
    }

    /// Whether the check has been passed.
    pub fn passed(&self) -> bool {
        self.stage == Stage::Passed
    }

    /// SS-20: seal the seed under a new random data key. Only after the
    /// check is passed. The setup is kept (the caller drops it once the sealed
    /// seed is stored), so a refusal of the sealing loses nothing.
    pub fn seal(&self, rng: &mut dyn KernelRandom) -> Result<Sealed, StarterRefusal> {
        if self.stage != Stage::Passed {
            return Err(StarterRefusal::Stage);
        }
        seal_seed(&self.seed, rng)
    }
}

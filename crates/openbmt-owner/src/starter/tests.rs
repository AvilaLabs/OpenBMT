//! The starter wallet (`docs/START-SIMPLE-RULES.md`, SS-01 to SS-50).
//!
//! TEST ONLY, NEVER USE: every key here comes from fixed bytes or from the
//! published BIP39 and BIP84 vectors. The vectors of SS-03 and SS-05 were
//! made by an independent implementation in Python (hashlib only: the
//! mix, the word list, PBKDF2, BIP32, the secp256k1 sums and Bech32), and
//! the first of them is the one the BIP84 specification publishes.

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{ChildNumber, Xpriv};
use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, WPubkeyHash,
    Witness,
};
use openbmt_core::font::{FIRST, GLYPHS, GLYPH_H, GLYPH_W};
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::ImportedWallet;

use super::*;

// ---- sources of bytes ---------------------------------------------------------

/// A source of bytes for a test: the given reads first, then a chain of
/// hashes, so that every later read is new.
struct Source {
    given: Vec<[u8; 32]>,
    next: u32,
    reads: usize,
}

impl Source {
    fn chain() -> Self {
        Self::fixed(&[])
    }
    fn fixed(given: &[[u8; 32]]) -> Self {
        Self {
            given: given.iter().rev().copied().collect(),
            next: 0,
            reads: 0,
        }
    }
}

impl KernelRandom for Source {
    fn read32(&mut self) -> Option<[u8; KERNEL_BYTES]> {
        self.reads += 1;
        if let Some(g) = self.given.pop() {
            return Some(g);
        }
        self.next += 1;
        Some(sha256::Hash::hash(&self.next.to_le_bytes()).to_byte_array())
    }
}

/// A source that has nothing to give.
struct Dry;

impl KernelRandom for Dry {
    fn read32(&mut self) -> Option<[u8; KERNEL_BYTES]> {
        None
    }
}

fn counting(start: u8) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = start.wrapping_add(i as u8);
    }
    out
}

/// The kernel bytes the Python vectors use: 1, 2, ... 32.
fn kernel_a() -> [u8; 32] {
    counting(1)
}

/// `(i * 7 + 3) mod 256`.
fn kernel_c() -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = ((i * 7 + 3) % 256) as u8;
    }
    out
}

fn secp() -> Secp256k1<bitcoin::secp256k1::All> {
    Secp256k1::new()
}

fn words_of(seed: &Seed) -> Vec<&'static str> {
    seed.mnemonic().unwrap().words().collect()
}

fn text(seed: &Seed) -> String {
    words_of(seed).join(" ")
}

// ---- SS-01, SS-02 ---------------------------------------------------------------

#[test]
fn ss_01_the_source_is_read_twice_and_the_first_read_is_used() {
    let mut s = Source::fixed(&[kernel_a(), kernel_c()]);
    let got = checked_read(&mut s).unwrap();
    assert_eq!(got, kernel_a());
    assert_eq!(s.reads, 2);
}

#[test]
fn ss_01_a_source_with_nothing_is_refused() {
    assert_eq!(checked_read(&mut Dry), Err(StarterRefusal::Random));
    assert_eq!(
        make_seed(&mut Dry, &[]).unwrap_err(),
        StarterRefusal::Random
    );
    // A second read that fails is refused too.
    struct Once(Option<[u8; 32]>);
    impl KernelRandom for Once {
        fn read32(&mut self) -> Option<[u8; 32]> {
            self.0.take()
        }
    }
    assert_eq!(
        checked_read(&mut Once(Some(kernel_a()))),
        Err(StarterRefusal::Random)
    );
}

#[test]
fn ss_02_two_reads_that_are_the_same_are_refused() {
    let mut s = Source::fixed(&[kernel_a(), kernel_a()]);
    assert_eq!(checked_read(&mut s), Err(StarterRefusal::RandomRepeat));
    let mut s = Source::fixed(&[kernel_a(), kernel_a()]);
    assert!(Setup::begin(&mut s, &[]).is_err());
}

#[test]
fn ss_02_a_read_of_all_zero_bytes_is_refused_whichever_read_it_is() {
    let zero = [0u8; 32];
    assert_eq!(
        checked_read(&mut Source::fixed(&[zero, kernel_a()])),
        Err(StarterRefusal::RandomWeak)
    );
    assert_eq!(
        checked_read(&mut Source::fixed(&[kernel_a(), zero])),
        Err(StarterRefusal::RandomWeak)
    );
    assert_eq!(
        make_seed(&mut Source::fixed(&[zero, zero]), &[]).unwrap_err(),
        StarterRefusal::RandomWeak
    );
}

#[test]
fn ss_02_a_read_of_one_byte_repeated_is_refused() {
    for b in [1u8, 0x5a, 0xff] {
        let same = [b; 32];
        assert_eq!(
            checked_read(&mut Source::fixed(&[same, kernel_c()])),
            Err(StarterRefusal::RandomWeak),
            "byte {b}"
        );
    }
    // One different byte is enough to pass the shape check.
    let mut nearly = [9u8; 32];
    nearly[31] = 10;
    assert_eq!(
        checked_read(&mut Source::fixed(&[nearly, kernel_a()])),
        Ok(nearly)
    );
}

#[test]
fn ss_02_a_refusal_makes_no_key() {
    let mut s = Source::fixed(&[[7u8; 32], [8u8; 32]]);
    assert!(matches!(
        Setup::begin(&mut s, &[1, 2]),
        Err(StarterRefusal::RandomWeak)
    ));
}

// ---- SS-03 -----------------------------------------------------------------------

/// Entropy of the Python mix with the starter tag, and of the same mix with
/// the stick's tag (SK-20).
#[test]
fn ss_03_the_mix_is_the_sticks_with_the_starter_tag() {
    let cases: [(&[u8; 32], &[u8], &str, &str); 3] = [
        (
            &counting(1),
            &[],
            "68b9953e6f287a6ed3b277b92dcd754a",
            "26facc3ff21c5e14a39a1063d9acef02",
        ),
        (
            &counting(1),
            &[1, 2, 3, 4, 5, 6, 6, 5, 4, 3],
            "7bd7c1fed9f5dd0a5c0e0920d36473ff",
            "50a3f3f19e2bd90a531c3aadde7f0226",
        ),
        (
            &kernel_c(),
            &[6],
            "6e3914cde6bc545044b39cc883d8b042",
            "31efb3497dafde5fd2b73ea0e948688d",
        ),
    ];
    for (kernel, rolls, starter, stick) in cases {
        let got = openbmt_core::mix::mix(STARTER_TAG, kernel, rolls).unwrap();
        assert_eq!(hex(&got), starter);
        let theirs = openbmt_core::mix::mix(b"OpenBMT/stick/v1", kernel, rolls).unwrap();
        assert_eq!(hex(&theirs), stick);
        // The seed the library makes from those bytes is the starter's.
        let mut s = Source::fixed(&[*kernel, [3u8; 32]]);
        // [3; 32] is one byte repeated: the second read must be a different
        // shape, so give it a real one.
        s.given = vec![counting(200), *kernel];
        let seed = make_seed(&mut s, rolls).unwrap();
        assert_eq!(hex(seed.entropy()), starter);
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn ss_03_rolls_change_the_key_and_a_bad_roll_is_refused() {
    let a = make_seed(&mut Source::fixed(&[kernel_a(), counting(9)]), &[]).unwrap();
    let b = make_seed(&mut Source::fixed(&[kernel_a(), counting(9)]), &[1]).unwrap();
    let c = make_seed(&mut Source::fixed(&[kernel_a(), counting(9)]), &[1, 1]).unwrap();
    assert_ne!(a.entropy(), b.entropy());
    assert_ne!(b.entropy(), c.entropy());
    for bad in [&[0u8][..], &[7], &[1, 2, 9], b"1"] {
        assert_eq!(
            make_seed(&mut Source::fixed(&[kernel_a(), counting(9)]), bad).unwrap_err(),
            StarterRefusal::Rolls,
            "{bad:?}"
        );
    }
}

// ---- SS-04, SS-05 ----------------------------------------------------------------

#[test]
fn ss_04_the_wallet_is_bip84_account_zero_with_no_passphrase() {
    // The BIP84 specification's wallet: 16 zero bytes, the abandon words.
    let seed = Seed::from_entropy([0u8; 16]);
    assert_eq!(
        text(&seed),
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
    );
    let wallet = seed.wallet().unwrap();
    assert_eq!(
        wallet.receive_address(0).unwrap().to_string(),
        "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu"
    );
    assert_eq!(
        wallet.receive_address(1).unwrap().to_string(),
        "bc1qnjg0jd8228aq7egyzacy8cys3knf9xvrerkf9g"
    );
    assert_eq!(wallet.fingerprint(), "73c5da0a");
    assert!(wallet.description().contains("73c5da0a/84h/0h/0h"));
    assert!(wallet.description().starts_with("wpkh("));
    // The description reads back as the same wallet, and is not another
    // account.
    assert_eq!(StarterWallet::parse(wallet.description()).unwrap(), wallet);
    // The words are 12.
    assert_eq!(words_of(&seed).len(), STARTER_WORDS);
}

#[test]
fn ss_05_fixed_bytes_and_rolls_give_the_exact_words_fingerprint_and_addresses() {
    // From the independent Python implementation.
    struct Vector {
        kernel: [u8; 32],
        rolls: &'static [u8],
        words: &'static str,
        fingerprint: &'static str,
        first: &'static str,
        second: &'static str,
    }
    let vectors = [
        Vector {
            kernel: counting(1),
            rolls: &[],
            words:
                "hamster smile exhibit tattoo marble danger excite exclude rice hover front nice",
            fingerprint: "ae8be9b4",
            first: "bc1q9lstj832v6pwwf88j9wt85zcdk2kgy0rcnm4gu",
            second: "bc1qp3lex4lc6pksl3d5jj8puevxjrkvdwhmytg5ws",
        },
        Vector {
            kernel: counting(1),
            rolls: &[1, 2, 3, 4, 5, 6, 6, 5, 4, 3],
            words: "knock safe leisure recycle fruit lunch ice scorpion camp open brother zero",
            fingerprint: "ba7e95cd",
            first: "bc1q7xpkht8uju94z2s86h47tlg2wezftw3fftukaa",
            second: "bc1q0jyffm58v23lxa05uq3eq80pmwcgaxp3jcx2ce",
        },
        Vector {
            kernel: kernel_c(),
            rolls: &[6],
            words: "hotel silly cricket sniff shed choice barrel inform silk burst clown lumber",
            fingerprint: "0f4f929f",
            first: "bc1qqnc9ektuqg4y0ysekp4hqnaduw4w0nv242mv9d",
            second: "bc1qf6dheh02rwd9dp0j4kg48gpza8f73chg5qsk2m",
        },
    ];
    for v in vectors {
        // The path of the app: Setup::begin, from the same two reads.
        let mut s = Source::fixed(&[v.kernel, counting(77)]);
        let setup = Setup::begin(&mut s, v.rolls).unwrap();
        assert_eq!(text(&setup.seed), v.words);
        let w = setup.wallet();
        assert_eq!(w.fingerprint(), v.fingerprint);
        assert_eq!(w.receive_address(0).unwrap().to_string(), v.first);
        assert_eq!(w.receive_address(1).unwrap().to_string(), v.second);
    }
}

// ---- SS-10, SS-11 ----------------------------------------------------------------

#[test]
fn ss_10_positions_follow_the_byte_rule() {
    // 12, 24, 36 are all 0 (mod 12): the first is kept, then 5, then the
    // lowest free place, 1. Sorted: 0, 1, 5.
    assert_eq!(pick_positions(&[12, 24, 36, 5]), [0, 1, 5]);
    // 100 mod 12 = 4, 200 mod 12 = 8; 255 skipped; then the lowest free
    // place, 0. Sorted: 0, 4, 8.
    assert_eq!(pick_positions(&[255, 100, 255, 100, 200]), [0, 4, 8]);
    assert_eq!(pick_positions(&[0, 1, 2]), [0, 1, 2]);
    assert_eq!(pick_positions(&[11, 5, 7]), [5, 7, 11]);
    assert_eq!(pick_positions(&[251, 252, 253, 254, 255]), [0, 1, 11]);
    assert_eq!(pick_positions(&[13, 25, 37, 49]), [0, 1, 2]);
    assert_eq!(pick_positions(&[13, 14, 15, 16]), [1, 2, 3]);
    assert_eq!(pick_positions(&[]), [0, 1, 2]);
}

#[test]
fn ss_10_the_positions_come_from_fresh_kernel_bytes() {
    let mut setup = Setup::begin(&mut Source::chain(), &[]).unwrap();
    // 32 fresh bytes: 7, 19 (7 again), 3, then the rest.
    let mut bytes = [255u8; 32];
    bytes[0] = 7;
    bytes[1] = 19;
    bytes[2] = 3;
    bytes[3] = 11;
    let asked = setup
        .start_check(&mut Source::fixed(&[bytes, counting(4)]))
        .unwrap();
    // Positions 3, 7 and 11, shown 1 to 12.
    assert_eq!(asked, [4, 8, 12]);
    // Two checks pick differently for different bytes, and a source that is
    // weak is refused.
    assert_eq!(
        setup.start_check(&mut Source::fixed(&[[0u8; 32], counting(4)])),
        Err(StarterRefusal::RandomWeak)
    );
}

/// Types the letters of `word` on a setup's keypad and returns what the
/// last key said.
fn type_on(setup: &mut Setup, word: &str, letters: usize) -> Typed {
    let mut last = None;
    for b in word.bytes().take(letters) {
        last = Some(setup.key(Key::from_byte(b).unwrap()).unwrap());
    }
    last.unwrap()
}

fn setup_checking() -> (Setup, [usize; 3]) {
    let mut setup = Setup::begin(&mut Source::fixed(&[kernel_a(), counting(77)]), &[]).unwrap();
    let asked = setup.start_check(&mut Source::chain()).unwrap();
    (setup, asked)
}

#[test]
fn ss_10_the_three_positions_are_answered_by_the_first_four_letters() {
    let (mut setup, asked) = setup_checking();
    let words = words_of(&setup.seed);
    for (i, pos) in asked.iter().enumerate() {
        let word = words[pos - 1];
        let t = type_on(&mut setup, word, 4);
        assert!(t.accepted, "{word}");
        assert_eq!(t.typed, 4.min(word.len()));
        assert_eq!(t.step, i);
        let t = setup.key(Key::Enter).unwrap();
        assert_eq!(t.step, i + 1);
        assert_eq!(t.typed, 0);
        assert!(!t.failed);
        assert_eq!(t.finished, i == 2);
    }
    assert!(setup.passed());
}

#[test]
fn ss_10_a_letter_case_and_the_whole_word_are_taken() {
    let (mut setup, asked) = setup_checking();
    let words = words_of(&setup.seed);
    let word = words[asked[0] - 1].to_uppercase();
    let t = type_on(&mut setup, &word, word.len());
    assert!(t.accepted);
    assert_eq!(t.typed, word.len());
}

#[test]
fn ss_10_a_word_is_not_accepted_before_its_first_four_letters() {
    let (mut setup, asked) = setup_checking();
    let words = words_of(&setup.seed);
    let word = words[asked[0] - 1];
    if word.len() >= 4 {
        let t = type_on(&mut setup, word, 3);
        assert!(!t.accepted);
        let t = setup.key(Key::Letter(word.as_bytes()[3])).unwrap();
        assert!(t.accepted);
    }
}

#[test]
fn ss_10_a_wrong_answer_shows_the_words_again_and_asks_again() {
    let (mut setup, asked) = setup_checking();
    let words = words_of(&setup.seed);
    let right = words[asked[0] - 1];
    // A word of the list that is not the one asked.
    let wrong = if right == "abandon" { "zoo" } else { "abandon" };
    let t = type_on(&mut setup, wrong, wrong.len());
    assert!(!t.accepted);
    let t = setup.key(Key::Enter).unwrap();
    assert!(t.failed);
    assert!(!t.finished);
    // The keys are no longer taken, and a new check is refused until the
    // words have been shown again.
    assert_eq!(setup.key(Key::Letter(b'a')), Err(StarterRefusal::Stage));
    assert_eq!(
        setup.start_check(&mut Source::chain()),
        Err(StarterRefusal::Stage)
    );
    // Shown again, the check starts again with new positions.
    setup.page(0, 2).unwrap();
    let asked = setup.start_check(&mut Source::chain()).unwrap();
    assert_eq!(asked.len(), 3);
    let t = setup.key(Key::Backspace).unwrap();
    assert_eq!(t.typed, 0);
    assert!(!t.failed);
}

#[test]
fn ss_10_the_words_are_shown_only_before_the_check_is_passed() {
    let mut setup = Setup::begin(&mut Source::chain(), &[]).unwrap();
    for p in 0..PAGES {
        setup.page(p, 2).unwrap();
    }
    assert_eq!(setup.page(PAGES, 2).unwrap_err(), StarterRefusal::Page);
    // Not while the check is being answered.
    let asked = setup.start_check(&mut Source::chain()).unwrap();
    assert_eq!(setup.page(0, 2).unwrap_err(), StarterRefusal::Stage);
    // Not after it is passed.
    let words = words_of(&setup.seed);
    for pos in asked {
        type_on(&mut setup, words[pos - 1], 4);
        setup.key(Key::Enter).unwrap();
    }
    assert!(setup.passed());
    assert_eq!(setup.page(0, 2).unwrap_err(), StarterRefusal::Stage);
    // And the keypad of the check takes nothing more.
    assert_eq!(setup.key(Key::Letter(b'a')), Err(StarterRefusal::Stage));
}

#[test]
fn ss_10_nothing_is_sealed_after_a_wrong_answer() {
    let (mut setup, asked) = setup_checking();
    let words = words_of(&setup.seed);
    let right = words[asked[0] - 1];
    let wrong = if right == "zoo" { "abandon" } else { "zoo" };
    type_on(&mut setup, wrong, wrong.len());
    assert!(setup.key(Key::Enter).unwrap().failed);
    assert_eq!(
        setup.seal(&mut Source::chain()).unwrap_err(),
        StarterRefusal::Stage
    );
}

#[test]
fn ss_10_nothing_is_sealed_before_the_check_is_passed() {
    let setup = Setup::begin(&mut Source::chain(), &[]).unwrap();
    assert_eq!(
        setup.seal(&mut Source::chain()).unwrap_err(),
        StarterRefusal::Stage
    );
    let (setup, _) = setup_checking();
    assert_eq!(
        setup.seal(&mut Source::chain()).unwrap_err(),
        StarterRefusal::Stage
    );
}

/// Reads the text of an image back with the font, cell by cell: the lines
/// of a page, as the image shows them.
fn read_image(img: &Image, scale: usize, lines: usize, cols: usize) -> Vec<String> {
    let margin = GLYPH_W * scale;
    let pitch = (GLYPH_H + 8) * scale;
    let w = img.width as usize;
    let mut out = Vec::new();
    for line in 0..lines {
        let mut s = String::new();
        for col in 0..cols {
            let mut rows = [0u16; GLYPH_H];
            for (row, bits) in rows.iter_mut().enumerate() {
                for bit in 0..GLYPH_W {
                    let x = margin + (col * GLYPH_W + bit) * scale;
                    let y = margin + line * pitch + row * scale;
                    if img.pixels[y * w + x] == 0 {
                        *bits |= 1 << (GLYPH_W - 1 - bit);
                    }
                }
            }
            let at = GLYPHS.iter().position(|g| *g == rows).unwrap();
            s.push(char::from(FIRST + at as u8));
        }
        out.push(s.trim_end().to_string());
    }
    out
}

#[test]
fn ss_11_a_screen_is_an_image_of_four_numbered_words_drawn_by_the_core() {
    let mut setup = Setup::begin(&mut Source::fixed(&[kernel_a(), counting(77)]), &[]).unwrap();
    let words = words_of(&setup.seed);
    let scale = 2usize;
    for page in 0..PAGES {
        let img = setup.page(page, scale as u32).unwrap();
        let want: Vec<String> = (0..4)
            .map(|i| {
                let n = page * 4 + i;
                format!("{:>2} {}", n + 1, words[n])
            })
            .collect();
        let cols = want.iter().map(|l| l.len()).max().unwrap();
        assert_eq!(
            img.width as usize,
            2 * GLYPH_W * scale + cols * GLYPH_W * scale
        );
        assert_eq!(
            img.height as usize,
            2 * GLYPH_W * scale + 4 * (GLYPH_H + 8) * scale
        );
        assert_eq!(img.pixels.len(), (img.width * img.height) as usize);
        // One byte each pixel: ink 0, paper 255, nothing between.
        assert!(img.pixels.iter().all(|p| *p == 0 || *p == 255));
        assert!(img.pixels.contains(&0));
        // The image reads back, with the font, as the four lines.
        let got = read_image(&img, scale, 4, cols);
        assert_eq!(got, want);
    }
}

#[test]
fn ss_11_an_image_is_wiped_not_debugged_and_scaled_within_limits() {
    let mut setup = Setup::begin(&mut Source::chain(), &[]).unwrap();
    let words = words_of(&setup.seed);
    let img = setup.page(0, 1).unwrap();
    let shown = format!("{img:?}");
    for w in &words {
        assert!(!shown.contains(w));
    }
    // A scale of 0 is 1, a scale beyond the limit is the limit.
    let small = setup.page(0, 0).unwrap();
    assert_eq!(small.width, img.width);
    let big = setup.page(0, 1000).unwrap();
    assert_eq!(big.width, img.width * 16);
    let mut pixels = big.pixels.clone();
    wipe(&mut pixels);
    assert!(pixels.iter().all(|p| *p == 0));
}

#[test]
fn ss_11_the_keypad_gives_counts_and_a_yes_and_never_the_letters() {
    let (mut setup, asked) = setup_checking();
    let words = words_of(&setup.seed);
    let t = type_on(&mut setup, words[asked[0] - 1], 2);
    // `Typed` is four numbers and flags: its debug form holds no letter of
    // the word.
    assert_eq!(
        format!("{t:?}"),
        "Typed { typed: 2, accepted: false, step: 0, finished: false, failed: false }"
    );
    let shown = format!("{setup:?}");
    for w in &words {
        assert!(!shown.contains(w));
    }
    // Backspace takes back a letter; the count follows.
    assert_eq!(setup.key(Key::Backspace).unwrap().typed, 1);
    assert_eq!(setup.key(Key::Backspace).unwrap().typed, 0);
    assert_eq!(setup.key(Key::Backspace).unwrap().typed, 0);
    // No more than the longest word's letters are held.
    let t = type_on(&mut setup, "zzzzzzzzzzzz", 12);
    assert_eq!(t.typed, 8);
}

#[test]
fn ss_11_only_letters_backspace_and_enter_are_keys() {
    assert_eq!(Key::from_byte(b'a'), Ok(Key::Letter(b'a')));
    assert_eq!(Key::from_byte(b'Q'), Ok(Key::Letter(b'q')));
    assert_eq!(Key::from_byte(8), Ok(Key::Backspace));
    assert_eq!(Key::from_byte(10), Ok(Key::Enter));
    assert_eq!(Key::from_byte(13), Ok(Key::Enter));
    for b in [b'1', b' ', 0, 27, 127, 0xc3, b'-'] {
        assert_eq!(Key::from_byte(b), Err(StarterRefusal::Key), "{b}");
    }
}

// ---- SS-20 -----------------------------------------------------------------------

fn passed_setup(rng_seed: u8) -> Setup {
    let mut setup =
        Setup::begin(&mut Source::fixed(&[counting(rng_seed), counting(99)]), &[]).unwrap();
    let asked = setup.start_check(&mut Source::chain()).unwrap();
    let words = words_of(&setup.seed);
    for pos in asked {
        type_on(&mut setup, words[pos - 1], 4);
        setup.key(Key::Enter).unwrap();
    }
    assert!(setup.passed());
    setup
}

fn sealed_starter() -> (Sealed, StarterWallet, Seed) {
    let setup = passed_setup(1);
    let wallet = setup.wallet().clone();
    let copy = Seed::from_entropy(*setup.seed.entropy());
    let sealed = setup.seal(&mut Source::chain()).unwrap();
    (sealed, wallet, copy)
}

#[test]
fn ss_20_the_seed_is_sealed_and_opens_with_its_data_key() {
    let (sealed, wallet, seed) = sealed_starter();
    assert_eq!(sealed.data_key.as_bytes().len(), DATA_KEY_BYTES);
    // Version, nonce, 16 bytes of seed and a 16-byte tag.
    assert_eq!(sealed.sealed.len(), 1 + 12 + 16 + 16);
    assert_eq!(sealed.sealed[0], 1);
    let opened = wallet_of_sealed(&sealed.sealed, &sealed.data_key).unwrap();
    assert_eq!(opened, wallet);
    // The entropy and the key are not in the blob in the clear.
    let blob = &sealed.sealed;
    assert!(!blob.windows(16).any(|w| w == seed.entropy()));
    assert!(!blob
        .windows(32)
        .any(|w| w == sealed.data_key.as_bytes().as_slice()));
}

#[test]
fn ss_20_a_wrong_key_or_a_changed_byte_opens_nothing() {
    let (sealed, _, _) = sealed_starter();
    let wrong = DataKey::from_slice(&[0x42; 32]).unwrap();
    assert_eq!(
        wallet_of_sealed(&sealed.sealed, &wrong).unwrap_err(),
        StarterRefusal::Open
    );
    for i in 0..sealed.sealed.len() {
        let mut bad = sealed.sealed.clone();
        bad[i] ^= 1;
        assert_eq!(
            wallet_of_sealed(&bad, &sealed.data_key).unwrap_err(),
            StarterRefusal::Open,
            "byte {i}"
        );
    }
    for n in 0..sealed.sealed.len() {
        assert!(wallet_of_sealed(&sealed.sealed[..n], &sealed.data_key).is_err());
    }
    let mut longer = sealed.sealed.clone();
    longer.push(0);
    assert!(wallet_of_sealed(&longer, &sealed.data_key).is_err());
}

#[test]
fn ss_20_the_data_key_is_32_bytes() {
    for n in [0usize, 1, 31, 33, 64] {
        assert_eq!(
            DataKey::from_slice(&vec![1u8; n]).unwrap_err(),
            StarterRefusal::DataKey,
            "{n}"
        );
    }
    assert!(DataKey::from_slice(&[1u8; 32]).is_ok());
    assert_eq!(
        format!("{:?}", DataKey::from_slice(&[1u8; 32]).unwrap()),
        "DataKey(<hidden>)"
    );
}

#[test]
fn ss_20_each_sealing_makes_a_new_key_and_a_new_blob() {
    let a = passed_setup(1).seal(&mut Source::chain()).unwrap();
    let mut other = Source::chain();
    other.next = 1000;
    let b = passed_setup(1).seal(&mut other).unwrap();
    assert_ne!(a.data_key.as_bytes(), b.data_key.as_bytes());
    assert_ne!(a.sealed, b.sealed);
}

#[test]
fn ss_20_the_key_and_the_nonce_pass_the_checks_of_ss_02() {
    let setup = passed_setup(1);
    assert_eq!(
        setup
            .seal(&mut Source::fixed(&[[0u8; 32], counting(5)]))
            .unwrap_err(),
        StarterRefusal::RandomWeak
    );
    let setup = passed_setup(1);
    assert_eq!(setup.seal(&mut Dry).unwrap_err(), StarterRefusal::Random);
}

#[test]
fn ss_20_buffers_are_overwritten_with_zeros() {
    let mut a = [0xabu8; 32];
    wipe(&mut a);
    assert_eq!(a, [0u8; 32]);
    let mut v = vec![1u8, 2, 3];
    wipe(&mut v);
    assert_eq!(v, vec![0, 0, 0]);
}

// ---- SS-23 -----------------------------------------------------------------------

fn type_restore(r: &mut Restore, words: &[&str]) {
    for w in words {
        for b in w.bytes() {
            r.key(Key::from_byte(b).unwrap()).unwrap();
        }
        r.key(Key::Enter).unwrap();
    }
}

#[test]
fn ss_23_twelve_typed_words_of_the_wallet_are_sealed_again() {
    let (old, wallet, seed) = sealed_starter();
    let mut again = Source::chain();
    again.next = 5_000;
    let mut r = Restore::new();
    type_restore(&mut r, &words_of(&seed));
    let t = r.key(Key::Backspace).unwrap();
    // Backspace with nothing typed takes the last word back; type it again.
    assert_eq!(t.step, 11);
    type_restore(&mut r, &words_of(&seed)[11..]);
    let fresh = r.finish(wallet.description(), &mut again).unwrap();
    // The new seal opens to the same wallet, and is not the old one.
    assert_eq!(
        wallet_of_sealed(&fresh.sealed, &fresh.data_key).unwrap(),
        wallet
    );
    assert_ne!(fresh.sealed, old.sealed);
    assert_ne!(fresh.data_key.as_bytes(), old.data_key.as_bytes());
    // The old data key does not open the new seal.
    assert!(wallet_of_sealed(&fresh.sealed, &old.data_key).is_err());
}

#[test]
fn ss_23_the_words_are_taken_by_unique_start_and_not_when_unclear() {
    let mut r = Restore::new();
    // "aban" is the start of one word only.
    let mut last = None;
    for b in "aban".bytes() {
        last = Some(r.key(Key::from_byte(b).unwrap()).unwrap());
    }
    assert!(last.unwrap().accepted);
    assert_eq!(last.unwrap().typed, 4);
    // "a" starts many words: not accepted, and enter takes nothing.
    r.key(Key::Backspace).unwrap();
    r.key(Key::Backspace).unwrap();
    r.key(Key::Backspace).unwrap();
    let t = r.key(Key::Backspace).unwrap();
    assert_eq!(t.typed, 0);
    let t = r.key(Key::Letter(b'a')).unwrap();
    assert!(!t.accepted);
    let t = r.key(Key::Enter).unwrap();
    assert_eq!(t.step, 0);
    assert_eq!(t.typed, 1);
    // Letters that are no word are never accepted.
    let t = type_restore_one(&mut r, "xyzzy");
    assert!(!t.accepted);
    assert_eq!(t.step, 0);
}

fn type_restore_one(r: &mut Restore, letters: &str) -> Typed {
    let mut last = r.key(Key::Backspace).unwrap();
    for _ in 0..8 {
        last = r.key(Key::Backspace).unwrap();
    }
    for b in letters.bytes() {
        last = r.key(Key::from_byte(b).unwrap()).unwrap();
    }
    last
}

#[test]
fn ss_23_other_words_a_bad_check_word_and_too_few_are_refused() {
    let (_, wallet, _) = sealed_starter();
    // Valid words of another wallet.
    let other = Seed::from_entropy([7u8; 16]);
    let mut r = Restore::new();
    type_restore(&mut r, &words_of(&other));
    assert_eq!(
        r.finish(wallet.description(), &mut Source::chain())
            .unwrap_err(),
        StarterRefusal::OtherWallet
    );
    // Twelve words of the list with a wrong check word.
    let mut r = Restore::new();
    type_restore(&mut r, &["abandon"; 12]);
    assert_eq!(
        r.finish(wallet.description(), &mut Source::chain())
            .unwrap_err(),
        StarterRefusal::Words
    );
    // Eleven.
    let mut r = Restore::new();
    type_restore(&mut r, &words_of(&other)[..11]);
    assert_eq!(
        r.finish(wallet.description(), &mut Source::chain())
            .unwrap_err(),
        StarterRefusal::Incomplete
    );
    // A description that is not a starter wallet's.
    let mut r = Restore::new();
    type_restore(&mut r, &words_of(&other));
    assert!(r.finish("nonsense", &mut Source::chain()).is_err());
}

#[test]
fn ss_23_twelve_words_are_all_it_takes_and_no_more() {
    let seed = Seed::from_entropy([7u8; 16]);
    let mut r = Restore::new();
    type_restore(&mut r, &words_of(&seed));
    let t = r.key(Key::Letter(b'a')).unwrap();
    assert!(t.finished);
    assert!(!t.accepted);
    let t = r.key(Key::Enter).unwrap();
    assert_eq!(t.step, 12);
    assert_eq!(format!("{r:?}"), "Restore(12 words)");
}

// ---- SS-30, SS-31, SS-42 -----------------------------------------------------------

const RATE: u64 = 2_000;

fn pay_to() -> ScriptBuf {
    ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([7; 20]))
}

/// A coin of the starter wallet: `sats` at `chain/index`, `confirmations`
/// deep.
fn coin_of(
    wallet: &StarterWallet,
    tag: u32,
    chain: Keychain,
    index: u32,
    sats: u64,
    confirmations: u32,
) -> Coin {
    coin_to(
        wallet.script(chain, index).unwrap(),
        tag,
        chain,
        index,
        sats,
        confirmations,
    )
}

fn coin_to(
    script: ScriptBuf,
    tag: u32,
    chain: Keychain,
    index: u32,
    sats: u64,
    confirmations: u32,
) -> Coin {
    let mut id = [0u8; 32];
    id[..4].copy_from_slice(&tag.to_le_bytes());
    let prev = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_byte_array(id),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(sats),
            script_pubkey: script,
        }],
    };
    Coin {
        outpoint: OutPoint {
            txid: prev.compute_txid(),
            vout: 0,
        },
        prev_tx: prev,
        vout: 0,
        chain,
        index,
        confirmations,
    }
}

/// Every input of `tx` is signed by the key of the coin it spends: the
/// signature verifies against the sighash worked here, and the key is the
/// coin's own.
fn all_signed(tx: &Transaction, coins: &[Coin]) {
    let secp = secp();
    let mut cache = SighashCache::new(tx);
    for (n, input) in tx.input.iter().enumerate() {
        let coin = coins
            .iter()
            .find(|c| c.outpoint == input.previous_output)
            .unwrap();
        let out = coin.output().unwrap();
        let w: Vec<&[u8]> = input.witness.iter().collect();
        assert_eq!(w.len(), 2, "a P2WPKH witness");
        let sig = bitcoin::ecdsa::Signature::from_slice(w[0]).unwrap();
        assert_eq!(sig.sighash_type, EcdsaSighashType::All);
        let key = bitcoin::PublicKey::from_slice(w[1]).unwrap();
        assert_eq!(
            ScriptBuf::new_p2wpkh(&key.wpubkey_hash().unwrap()),
            out.script_pubkey
        );
        let hash = cache
            .p2wpkh_signature_hash(n, &out.script_pubkey, out.value, EcdsaSighashType::All)
            .unwrap();
        secp.verify_ecdsa(
            &Message::from_digest(hash.to_byte_array()),
            &sig.signature,
            &key.inner,
        )
        .unwrap();
    }
}

struct Rig {
    sealed: Sealed,
    wallet: StarterWallet,
}

fn rig() -> Rig {
    let (sealed, wallet, _) = sealed_starter();
    Rig { sealed, wallet }
}

#[test]
fn ss_30_a_queued_refund_is_signed_with_the_starters_own_key() {
    let r = rig();
    let coins = vec![
        coin_of(&r.wallet, 1, Keychain::Receive, 0, 100_000, 6),
        coin_of(&r.wallet, 2, Keychain::Receive, 1, 80_000, 6),
    ];
    let paid = pay_refund(
        &r.sealed.sealed,
        &r.sealed.data_key,
        &coins,
        &pay_to(),
        70_000,
        RATE,
        0,
        1,
        800_000,
    )
    .unwrap();
    all_signed(&paid.tx, &coins);
    assert_eq!(paid.paid, 70_000);
    assert!(paid.fee > 0);
    assert!(paid.fee * 1000 >= RATE * paid.vsize);
    assert_eq!(paid.txid, paid.tx.compute_txid());
    // Only the refund and the change at the starter's change index 0.
    let change_script = r.wallet.script(Keychain::Change, 0).unwrap();
    let mut scripts: Vec<&ScriptBuf> = paid.tx.output.iter().map(|o| &o.script_pubkey).collect();
    scripts.sort();
    let refund_script = pay_to();
    let mut want = [&refund_script, &change_script];
    want.sort();
    assert_eq!(scripts, want.to_vec());
    let (index, sats) = paid.change.unwrap();
    assert_eq!(index, 0);
    let held: u64 = paid
        .tx
        .input
        .iter()
        .map(|i| {
            coins
                .iter()
                .find(|c| c.outpoint == i.previous_output)
                .unwrap()
                .output()
                .unwrap()
                .value
                .to_sat()
        })
        .sum();
    assert_eq!(held, paid.paid + sats + paid.fee);
    assert_eq!(paid.tx.lock_time, LockTime::from_height(800_000).unwrap());
    // The coins it spent are the ones listed.
    assert_eq!(paid.spent.len(), paid.tx.input.len());
}

#[test]
fn ss_30_a_wrong_data_key_or_sealed_seed_signs_nothing() {
    let r = rig();
    let coins = vec![coin_of(&r.wallet, 1, Keychain::Receive, 0, 100_000, 6)];
    let wrong = DataKey::from_slice(&[9u8; 32]).unwrap();
    assert_eq!(
        pay_refund(
            &r.sealed.sealed,
            &wrong,
            &coins,
            &pay_to(),
            1000,
            RATE,
            0,
            1,
            0
        )
        .unwrap_err(),
        StarterRefusal::Open
    );
    assert_eq!(
        pay_refund(
            &[],
            &r.sealed.data_key,
            &coins,
            &pay_to(),
            1000,
            RATE,
            0,
            1,
            0
        )
        .unwrap_err(),
        StarterRefusal::Open
    );
}

#[test]
fn ss_30_a_refund_the_coins_cannot_cover_is_refused_and_not_split() {
    let r = rig();
    let coins = vec![
        coin_of(&r.wallet, 1, Keychain::Receive, 0, 50_000, 6),
        coin_of(&r.wallet, 2, Keychain::Receive, 1, 40_000, 6),
    ];
    match pay_refund(
        &r.sealed.sealed,
        &r.sealed.data_key,
        &coins,
        &pay_to(),
        200_000,
        RATE,
        0,
        1,
        0,
    )
    .unwrap_err()
    {
        StarterRefusal::NotEnough { available, needed } => {
            assert_eq!(available, 90_000);
            assert!(needed > 200_000);
        }
        e => panic!("{e}"),
    }
    // Coins not deep enough are not counted.
    let shallow = vec![coin_of(&r.wallet, 3, Keychain::Receive, 0, 500_000, 0)];
    assert!(matches!(
        pay_refund(
            &r.sealed.sealed,
            &r.sealed.data_key,
            &shallow,
            &pay_to(),
            1000,
            RATE,
            0,
            1,
            0
        )
        .unwrap_err(),
        StarterRefusal::NotEnough { available: 0, .. }
    ));
}

#[test]
fn ss_30_a_coin_that_is_not_the_starters_is_refused() {
    let r = rig();
    // A coin whose script is somebody else's.
    let foreign = coin_to(pay_to(), 9, Keychain::Receive, 0, 100_000, 6);
    let e = pay_refund(
        &r.sealed.sealed,
        &r.sealed.data_key,
        &[foreign],
        &pay_to(),
        1000,
        RATE,
        0,
        1,
        0,
    )
    .unwrap_err();
    assert!(matches!(e, StarterRefusal::NotStarterCoin(_)), "{e}");
    // A coin of another starter wallet is not this one's.
    let other = Seed::from_entropy([5u8; 16]).wallet().unwrap();
    let theirs = coin_of(&other, 8, Keychain::Receive, 0, 100_000, 6);
    let e = pay_refund(
        &r.sealed.sealed,
        &r.sealed.data_key,
        &[theirs],
        &pay_to(),
        1000,
        RATE,
        0,
        1,
        0,
    )
    .unwrap_err();
    assert!(matches!(e, StarterRefusal::NotStarterCoin(_)), "{e}");
}

#[test]
fn ss_30_the_fee_rate_is_the_float_rule() {
    assert_eq!(fee_rate(Some(5_000), None), Ok(5_000));
    assert_eq!(fee_rate(Some(300), Some(9_999)), Ok(MIN_RATE_PER_KVB));
    assert_eq!(fee_rate(None, Some(4_000)), Ok(4_000));
    assert_eq!(fee_rate(None, Some(0)), Err(StarterRefusal::NoFeeRate));
    assert_eq!(fee_rate(None, None), Err(StarterRefusal::NoFeeRate));
}

#[test]
fn ss_30_a_transaction_that_pays_anything_else_fails_the_check() {
    let r = rig();
    let coins = vec![coin_of(&r.wallet, 1, Keychain::Receive, 0, 100_000, 6)];
    let paid = pay_refund(
        &r.sealed.sealed,
        &r.sealed.data_key,
        &coins,
        &pay_to(),
        30_000,
        RATE,
        0,
        1,
        0,
    )
    .unwrap();
    let change = r.wallet.script(Keychain::Change, 0).unwrap();
    let used: Vec<&Coin> = coins.iter().collect();
    let ok = |tx: &Transaction| {
        check_only_these(
            r.wallet.single(),
            &used,
            tx,
            &pay_to(),
            Some(30_000),
            &change,
            false,
        )
    };
    assert!(ok(&paid.tx).is_ok());
    // An extra output.
    let mut tx = paid.tx.clone();
    tx.output.push(TxOut {
        value: Amount::from_sat(1_000),
        script_pubkey: ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([8; 20])),
    });
    assert!(matches!(ok(&tx), Err(StarterRefusal::NotOnlyThese(_))));
    // The refund paid twice.
    let mut tx = paid.tx.clone();
    tx.output.push(tx.output[0].clone());
    assert!(ok(&tx).is_err());
    // A wrong amount.
    let mut tx = paid.tx.clone();
    for o in tx.output.iter_mut().filter(|o| o.script_pubkey == pay_to()) {
        o.value = Amount::from_sat(30_001);
    }
    assert!(ok(&tx).is_err());
    // An input that is not one of the coins.
    let mut tx = paid.tx.clone();
    tx.input[0].previous_output.vout = 5;
    assert!(ok(&tx).is_err());
    // No input at all.
    let mut tx = paid.tx.clone();
    tx.input.clear();
    assert!(ok(&tx).is_err());
    // A payment to the starter's own change.
    assert!(check_only_these(
        r.wallet.single(),
        &used,
        &paid.tx,
        &change,
        None,
        &change,
        false
    )
    .is_err());
    // A move that leaves a coin behind.
    let two = [
        coin_of(&r.wallet, 1, Keychain::Receive, 0, 100_000, 6),
        coin_of(&r.wallet, 2, Keychain::Receive, 1, 100_000, 6),
    ];
    let both: Vec<&Coin> = two.iter().collect();
    let one_input = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: two[0].outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(99_000),
            script_pubkey: pay_to(),
        }],
    };
    assert!(check_only_these(
        r.wallet.single(),
        &both,
        &one_input,
        &pay_to(),
        None,
        &change,
        true
    )
    .is_err());
}

fn new_wallet() -> ImportedWallet {
    let master = Xpriv::new_master(Network::Bitcoin, &[0x77; 32]).unwrap();
    let secp = secp();
    let path: bitcoin::bip32::DerivationPath = "m/84'/0'/0'".parse().unwrap();
    let account = master.derive_priv(&secp, &path).unwrap();
    let single = openbmt_core::wallet_single::SingleKeyWallet::new(
        master.fingerprint(&secp),
        0,
        bitcoin::bip32::Xpub::from_priv(&secp, &account),
    )
    .unwrap();
    ImportedWallet::read(single.canonical_string()).unwrap()
}

#[test]
fn ss_42_the_move_pays_the_new_wallets_derived_address_from_every_coin() {
    let r = rig();
    let new = new_wallet();
    let coins = vec![
        coin_of(&r.wallet, 1, Keychain::Receive, 0, 100_000, 6),
        coin_of(&r.wallet, 2, Keychain::Receive, 3, 80_000, 2),
        coin_of(&r.wallet, 3, Keychain::Change, 0, 60_000, 1),
    ];
    let moved = move_all(
        &r.sealed.sealed,
        &r.sealed.data_key,
        &coins,
        &new,
        4,
        RATE,
        1,
        800_000,
    )
    .unwrap();
    all_signed(&moved.tx, &coins);
    // One output, the new wallet's receive script 4, no change.
    assert_eq!(moved.tx.output.len(), 1);
    assert_eq!(
        moved.tx.output[0].script_pubkey,
        new.receive_script(4).unwrap()
    );
    assert_eq!(moved.tx.input.len(), 3);
    assert_eq!(moved.change, None);
    assert_eq!(moved.paid + moved.fee, 240_000);
    assert_eq!(moved.paid, moved.tx.output[0].value.to_sat());
    // A different index is a different address.
    assert_ne!(
        new.receive_script(4).unwrap(),
        new.receive_script(5).unwrap()
    );
    assert_eq!(
        receive_script_of(&new, 4).unwrap(),
        new.receive_script(4).unwrap()
    );
}

#[test]
fn ss_42_the_move_refuses_nothing_to_move_unconfirmed_coins_and_itself() {
    let r = rig();
    let new = new_wallet();
    assert_eq!(
        move_all(
            &r.sealed.sealed,
            &r.sealed.data_key,
            &[],
            &new,
            0,
            RATE,
            1,
            0
        )
        .unwrap_err(),
        StarterRefusal::NothingToMove
    );
    // One coin not confirmed: the move would leave it, so it waits.
    let coins = vec![
        coin_of(&r.wallet, 1, Keychain::Receive, 0, 100_000, 6),
        coin_of(&r.wallet, 2, Keychain::Receive, 1, 80_000, 0),
    ];
    assert_eq!(
        move_all(
            &r.sealed.sealed,
            &r.sealed.data_key,
            &coins,
            &new,
            0,
            RATE,
            1,
            0
        )
        .unwrap_err(),
        StarterRefusal::Unconfirmed
    );
    // To itself: not a move.
    let itself = r.wallet.imported();
    let coins = vec![coin_of(&r.wallet, 1, Keychain::Receive, 0, 100_000, 6)];
    assert!(matches!(
        move_all(
            &r.sealed.sealed,
            &r.sealed.data_key,
            &coins,
            &itself,
            0,
            RATE,
            1,
            0
        )
        .unwrap_err(),
        StarterRefusal::NotOnlyThese(_)
    ));
    // A wrong data key.
    let wrong = DataKey::from_slice(&[1u8; 32]).unwrap();
    assert_eq!(
        move_all(&r.sealed.sealed, &wrong, &coins, &new, 0, RATE, 1, 0).unwrap_err(),
        StarterRefusal::Open
    );
    // A coin that is not the starter's.
    let foreign = vec![coin_to(pay_to(), 9, Keychain::Receive, 0, 100_000, 6)];
    assert!(matches!(
        move_all(
            &r.sealed.sealed,
            &r.sealed.data_key,
            &foreign,
            &new,
            0,
            RATE,
            1,
            0
        )
        .unwrap_err(),
        StarterRefusal::NotStarterCoin(_)
    ));
}

// ---- SS-35 to SS-38, SS-43 ---------------------------------------------------------

const DAY: u64 = 24 * 60 * 60;

#[test]
fn ss_35_the_state_under_at_the_limit_and_past_the_date() {
    let move_by = 1_000 * DAY;
    let now = 990 * DAY;
    assert_eq!(limit_state(0, 100, now, move_by, None), LimitState::Under);
    assert_eq!(limit_state(99, 100, now, move_by, None), LimitState::Under);
    assert_eq!(
        limit_state(100, 100, now, move_by, None),
        LimitState::AtLimit
    );
    assert_eq!(
        limit_state(101, 100, now, move_by, None),
        LimitState::AtLimit
    );
    assert_eq!(
        limit_state(5, 100, move_by - 1, move_by, None),
        LimitState::Under
    );
    assert_eq!(
        limit_state(5, 100, move_by, move_by, None),
        LimitState::PastDate
    );
    assert_eq!(
        limit_state(5, 100, move_by + 9 * DAY, move_by, None),
        LimitState::PastDate
    );
    // At the limit and past the date: the limit is named first.
    assert_eq!(
        limit_state(100, 100, move_by, move_by, None),
        LimitState::AtLimit
    );
    // No limit set is a wallet at its limit.
    assert_eq!(limit_state(0, 0, now, move_by, None), LimitState::AtLimit);
}

#[test]
fn ss_37_not_yet_lasts_at_most_seven_days() {
    let now = 1_000_000;
    assert_eq!(MAX_NOT_YET_SECS, 7 * DAY);
    assert_eq!(not_yet_until(now, 3 * DAY), now + 3 * DAY);
    assert_eq!(not_yet_until(now, 7 * DAY), now + 7 * DAY);
    assert_eq!(not_yet_until(now, 30 * DAY), now + 7 * DAY);
    assert_eq!(not_yet_until(now, u64::MAX), now + 7 * DAY);
    assert_eq!(not_yet_until(u64::MAX, DAY), u64::MAX);
    let until = not_yet_until(now, 30 * DAY);
    // While it lasts, at the limit or past the date is "Not yet".
    assert_eq!(
        limit_state(100, 100, now, 10, Some(until)),
        LimitState::NotYet { until }
    );
    assert_eq!(
        limit_state(5, 100, now + DAY, 10, Some(until)),
        LimitState::NotYet { until }
    );
    // Under the limit and before the date, there is nothing to put off.
    assert_eq!(
        limit_state(5, 100, now, now + 1, Some(until)),
        LimitState::Under
    );
    // At its end, and after, the upgrade screen is back.
    assert_eq!(
        limit_state(100, 100, until, 10, Some(until)),
        LimitState::AtLimit
    );
    assert_eq!(
        limit_state(100, 100, until + 1, 10, Some(until)),
        LimitState::AtLimit
    );
    // A stored end further than 7 days away is not honoured.
    assert_eq!(
        limit_state(100, 100, now, 10, Some(now + 7 * DAY + 1)),
        LimitState::AtLimit
    );
    assert_eq!(
        limit_state(100, 100, now, 10, Some(now + 7 * DAY)),
        LimitState::NotYet {
            until: now + 7 * DAY
        }
    );
    assert_eq!(limit_state(100, 100, now, 10, Some(0)), LimitState::AtLimit);
}

#[test]
fn ss_37_while_not_yet_lasts_what_loosens_waits_and_what_tightens_goes() {
    use SettingsChange::*;
    let held = LimitState::NotYet { until: 5 };
    for c in [RaiseLimit, LaterDate, AddCounter, StopAboveOff, Other] {
        assert!(change_held(held, c), "{c:?}");
        assert!(change_held(LimitState::AtLimit, c), "{c:?}");
        assert!(change_held(LimitState::PastDate, c), "{c:?}");
        assert!(!change_held(LimitState::Under, c), "{c:?}");
    }
    for c in [LowerLimit, EarlierDate, StopAboveOn] {
        for s in [
            held,
            LimitState::AtLimit,
            LimitState::PastDate,
            LimitState::Under,
        ] {
            assert!(!change_held(s, c), "{c:?} {s:?}");
        }
    }
}

#[test]
fn ss_38_a_counter_offers_bitcoin_unless_stop_above_is_on_and_the_wallet_is_full() {
    // Off by default: sales never stop.
    assert!(counter_offers_bitcoin(false, 0, 100));
    assert!(counter_offers_bitcoin(false, 100, 100));
    assert!(counter_offers_bitcoin(false, u64::MAX, 100));
    assert!(counter_offers_bitcoin(false, 5, 0));
    // On: only while under the limit.
    assert!(counter_offers_bitcoin(true, 99, 100));
    assert!(!counter_offers_bitcoin(true, 100, 100));
    assert!(!counter_offers_bitcoin(true, 101, 100));
    assert!(!counter_offers_bitcoin(true, 0, 0));
}

#[test]
fn ss_43_the_seed_may_go_with_one_confirmation_of_the_move() {
    assert!(!seed_may_go(0));
    assert!(seed_may_go(1));
    assert!(seed_may_go(6));
}

// ---- the words ----------------------------------------------------------------------

#[test]
fn ss_50_every_state_has_a_placeholder_for_its_words_and_each_is_its_own() {
    let all = all_starter_words();
    assert!(all.len() >= 30);
    let mut seen = std::collections::BTreeSet::new();
    for (name, w) in &all {
        assert!(crate::own_say::is_placeholder(w), "{name}");
        assert!(w.heading.starts_with("WORDS_NEEDED_SS_"), "{name}");
        assert_eq!(w.heading, w.what, "{name}");
        assert!(w.code.is_some() || name.starts_with("Starter"), "{name}");
        seen.insert(w.heading.clone());
    }
    // Refusals with their own placeholder are all different.
    let refusals: std::collections::BTreeSet<&str> = all_refusals()
        .iter()
        .map(|(_, r)| r.placeholder())
        .collect();
    assert_eq!(refusals.len(), all_refusals().len());
    // Every constant of the module is some state's words.
    let source = std::fs::read_to_string(
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("src/starter/mod.rs"),
    )
    .unwrap();
    let declared: std::collections::BTreeSet<String> = source
        .lines()
        .filter_map(|l| l.strip_prefix("pub const WORDS_NEEDED_SS_"))
        .filter_map(|l| l.split(':').next())
        .map(|n| format!("WORDS_NEEDED_SS_{n}"))
        .collect();
    assert!(declared.len() >= 30);
    // The SS-13 screen and the SS-37 typed sentence are the app's.
    assert_eq!(
        declared, seen,
        "a constant that no state uses, or the reverse"
    );
}

#[test]
fn ss_50_a_refusal_names_its_rule_and_carries_the_detail() {
    let w = StarterRefusal::Plan("no coin".to_string()).words();
    assert_eq!(w.code, Some("SS-30"));
    assert_eq!(StarterRefusal::Random.rule(), "SS-01");
    assert_eq!(StarterRefusal::Unconfirmed.rule(), "SS-42");
    assert_eq!(StarterRefusal::Wrong.rule(), "SS-10");
    let text = StarterRefusal::NotEnough {
        available: 1,
        needed: 2,
    }
    .to_string();
    assert!(text.contains("SS-30"));
    // Nothing a refusal says holds a word of the key.
    assert!(state_words(LimitState::Under).is_none());
    assert!(state_words(LimitState::AtLimit).is_some());
}

#[test]
fn ss_20_the_secp_helpers_used_here_are_the_same_ones_the_library_uses() {
    // The account key of the starter wallet signs for the coin's own path:
    // m/84'/0'/0'/0/i and m/84'/0'/0'/1/i, nothing else.
    let seed = Seed::from_entropy([3u8; 16]);
    let opened = seed.open().unwrap();
    let secp = secp();
    for (chain, branch) in [(Keychain::Receive, 0u32), (Keychain::Change, 1)] {
        let child = opened
            .account
            .derive_priv(
                &secp,
                &[
                    ChildNumber::Normal { index: branch },
                    ChildNumber::Normal { index: 2 },
                ],
            )
            .unwrap();
        let key = bitcoin::PublicKey::new(child.private_key.public_key(&secp));
        assert_eq!(
            opened.wallet.script(chain, 2).unwrap(),
            ScriptBuf::new_p2wpkh(&key.wpubkey_hash().unwrap())
        );
    }
}

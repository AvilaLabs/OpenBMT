//! Tests named after the rules of `docs/STICK-RULES.md` (`sk_NN_...`). They
//! run on any computer; the whole boot is `stick/test-qemu.sh`.

use std::path::PathBuf;

mod common;

use openbmt_core::wallet_single::SingleKeyWallet;
use openbmt_qr::{decode_luma, Ecc};
use openbmt_stick::app::{App, Chance, Facts, Outcome};
use openbmt_stick::check::{answer_matches, pick_positions};
use openbmt_stick::draw::{pack, render, text_scale, wrap, write_row, Channel, PixelFormat};
use openbmt_stick::keys::{key_from_code, key_from_event, Key, EVENT_SIZE};
use openbmt_stick::mix::{dice_alone, dice_values, mix, MixError};
use openbmt_stick::modlist::{check_camera_list, check_list, CAMERA_ALLOWED, FORBIDDEN};
use openbmt_stick::mounts::{MOUNTS, NEVER};
use openbmt_stick::outputs::{public_of, words_of};
use openbmt_stick::screen::Screen;
use openbmt_stick::seedqr::compact_seedqr;
use openbmt_stick::typed::{check_words, complete_word, TypedError};
use openbmt_stick::words;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn bits_to_bytes(bits: &str) -> Vec<u8> {
    bits.as_bytes()
        .chunks(8)
        .map(|c| c.iter().fold(0u8, |a, b| (a << 1) | (b - b'0')))
        .collect()
}

fn root() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default())
}

fn entropy16(b: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(b);
    out
}

/// The kernel's bytes, from a fixed list that repeats.
struct Fixed(Vec<[u8; 32]>, usize);

impl Fixed {
    fn new(first: u8) -> Self {
        let a: [u8; 32] = std::array::from_fn(|i| first.wrapping_add(i as u8));
        let b: [u8; 32] = std::array::from_fn(|i| first.wrapping_mul(3).wrapping_add(i as u8 * 7));
        Fixed(vec![a, b], 0)
    }
}

impl Chance for Fixed {
    fn kernel_bytes(&mut self) -> [u8; 32] {
        let out = self.0[self.1 % self.0.len()];
        self.1 += 1;
        out
    }
}

fn facts() -> Facts {
    Facts {
        secure_boot: "1".into(),
        lockdown: "integrity".into(),
        interfaces: vec!["lo".into()],
        version: "0.1.0 test".into(),
        initrd_sha256: "ab".repeat(32),
    }
}

fn app() -> App<Fixed> {
    App::new(facts(), Fixed::new(1))
}

fn press(app: &mut App<Fixed>, s: &str) {
    for c in s.chars() {
        app.key(Key::Char(c));
    }
}

fn text(s: &Screen) -> String {
    s.lines.join("\n")
}

// The reference's 50 triplets and the page's expected values (KM-30, KM-31).
const TRIPLETS: &str = "136 235 425 512 124 432 523 456 456 652 532 431 651 643 314 361 145 162 145 265 365 641 143 532 531 541 146 614 461 364 126 324 256 216 214 231 126 325 524 541 265 326 436 416 634 631 461 245 356 352";
const DIGITS50: &str = "11651451144444631212242444153216166316542666543113";
const DICE_WORDS: &str =
    "dance summer quarter guitar magic arm response novel toe service light enough";

fn rolls_of(s: &str) -> Vec<u8> {
    s.bytes().filter(|b| *b != b' ').map(|b| b - b'0').collect()
}

#[test]
fn sk_20_the_mix_is_sha256_of_tag_zero_kernel_bytes_and_digits() {
    let kernel: [u8; 32] = std::array::from_fn(|i| i as u8);
    // Worked out outside this code (hashlib).
    assert_eq!(
        hex(&mix(&kernel, &[1, 2, 3, 4, 5, 6]).unwrap()),
        "92f9df27af6fb70ea7754f9b02dc7bc9"
    );
    // Either source alone: no rolls is allowed.
    assert_eq!(
        hex(&mix(&kernel, &[]).unwrap()),
        "0cc9e6cf13f00869a98efb30acf5488d"
    );
}

#[test]
fn sk_20_a_change_in_either_source_changes_the_key() {
    let a: [u8; 32] = [7; 32];
    let mut b = a;
    b[31] ^= 1;
    assert_ne!(mix(&a, &[1]).unwrap(), mix(&b, &[1]).unwrap());
    assert_ne!(mix(&a, &[1]).unwrap(), mix(&a, &[2]).unwrap());
    assert_ne!(mix(&a, &[1, 2]).unwrap(), mix(&a, &[2, 1]).unwrap());
}

#[test]
fn sk_20_rolls_outside_one_to_six_are_refused() {
    let k = [0u8; 32];
    assert_eq!(mix(&k, &[0]), Err(MixError::NotAFace));
    assert_eq!(mix(&k, &[1, 7]), Err(MixError::NotAFace));
}

#[test]
fn sk_21_any_number_of_rolls_is_taken_and_undo_removes_the_last() {
    let mut a = app();
    a.key(Key::Char('1'));
    press(&mut a, "123");
    assert!(text(&a.screen()).contains(&format!("{} 3", words::SK_21_ROLLS)));
    a.key(Key::Char('6'));
    a.key(Key::Backspace);
    a.key(Key::Backspace);
    let t = text(&a.screen());
    assert!(t.contains(&format!("{} 2", words::SK_21_ROLLS)), "{t}");
    assert!(t.contains("\n12\n"), "{t}");
    // The screen says 50 rolls are enough, and a 7 or a 0 is not a roll.
    assert!(t.contains(words::SK_21_ENOUGH));
    press(&mut a, "07");
    assert!(text(&a.screen()).contains(&format!("{} 2", words::SK_21_ROLLS)));
    // Zero rolls goes on too.
    let mut b = app();
    b.key(Key::Char('1'));
    assert_eq!(b.key(Key::Enter), Outcome::Continue);
    assert_eq!(b.screen().id, "words");
}

#[test]
fn sk_22_dice_alone_equals_the_key_maker_vectors() {
    let rolls = rolls_of(TRIPLETS);
    assert_eq!(rolls.len(), 150);
    let values = dice_values(&rolls).unwrap();
    assert_eq!(values.len(), 50);
    let digits: String = values.iter().map(|v| char::from(b'0' + v)).collect();
    assert_eq!(digits, DIGITS50);
    let entropy = dice_alone(&values).unwrap();
    // KM-31: the first 32 hex characters of the page's worked hash.
    assert_eq!(hex(&entropy), "375b26bdb3e85c176decb8e398860625");
    assert_eq!(words_of(&entropy).unwrap().join(" "), DICE_WORDS);
}

#[test]
fn sk_22_the_order_of_three_drops_repeats_and_an_open_three() {
    // KM-30's small cases.
    assert_eq!(
        dice_values(&rolls_of("123 132 231 321 312 213")).unwrap(),
        [1, 2, 3, 4, 5, 6]
    );
    assert_eq!(
        dice_values(&rolls_of("112 223 333 12")).unwrap(),
        Vec::<u8>::new()
    );
    assert_eq!(dice_values(&[1, 2, 3, 4, 5]).unwrap(), [1]);
    assert_eq!(dice_values(&[1, 2, 7]), Err(MixError::NotAFace));
}

#[test]
fn sk_22_exactly_fifty_values_are_needed() {
    assert_eq!(dice_alone(&[1; 49]), Err(MixError::NeedFiftyValues));
    assert_eq!(dice_alone(&[1; 51]), Err(MixError::NeedFiftyValues));
    assert_eq!(dice_alone(&[7; 50]), Err(MixError::NotAFace));
}

#[test]
fn sk_22_a_whole_dice_alone_run_on_the_screens_gives_the_page_words() {
    let mut a = app();
    a.key(Key::Char('2'));
    // Enter does nothing before 50 values are kept.
    press(&mut a, "136");
    a.key(Key::Enter);
    assert_eq!(a.screen().id, "dice");
    for d in rolls_of(TRIPLETS).iter().skip(3) {
        a.key(Key::Char(char::from(b'0' + d)));
    }
    let t = text(&a.screen());
    assert!(t.contains(&format!("{} 50/50", words::SK_22_VALUES)), "{t}");
    // A further roll is not taken once 50 values are kept.
    a.key(Key::Char('4'));
    assert!(text(&a.screen()).contains(&format!("{} 150", words::SK_22_ROLLS)));
    a.key(Key::Enter);
    assert_eq!(a.screen().id, "words");
    let shown = shown_words(&mut a);
    assert_eq!(shown.join(" "), DICE_WORDS);
}

#[test]
fn sk_22_undo_removes_the_last_roll() {
    let mut a = app();
    a.key(Key::Char('2'));
    press(&mut a, "1364");
    a.key(Key::Backspace);
    let t = text(&a.screen());
    assert!(t.contains(&format!("{} 3", words::SK_22_ROLLS)));
    assert!(t.contains(&format!("{} 1/50", words::SK_22_VALUES)));
}

/// Pages through the words screens and returns the 12 words shown.
fn shown_words(a: &mut App<Fixed>) -> Vec<String> {
    let mut out = Vec::new();
    for page in 0..3 {
        let s = a.screen();
        assert_eq!(s.id, "words");
        let title = format!("{} {}", words::SK_25_WORDS, page + 1);
        assert!(s.lines.contains(&title), "{:?}", s.lines);
        let mut four = 0;
        for line in &s.lines {
            if let Some((n, w)) = line.split_once(' ') {
                if let Ok(n) = n.parse::<usize>() {
                    assert_eq!(n, out.len() + 1);
                    out.push(w.to_string());
                    four += 1;
                }
            }
        }
        assert_eq!(four, 4, "four words a screen");
        a.key(Key::Enter);
    }
    assert_eq!(a.screen().id, "check");
    out
}

#[test]
fn sk_23_the_fixed_choices_are_twelve_words_and_the_bip84_account() {
    let words = words_of(&[0; 16]).unwrap();
    assert_eq!(words.len(), 12);
    let p = public_of(&words).unwrap();
    assert!(p.descriptor.contains("/84h/0h/0h]xpub"));
    assert!(p.descriptor.starts_with("wpkh(["));
    assert!(p.address.starts_with("bc1q"));
    // 24 words are refused.
    let long = vec!["abandon"; 24];
    assert!(public_of(&long).is_err());
}

#[test]
fn sk_25_positions_are_three_distinct_in_order_from_bytes_below_252() {
    assert_eq!(pick_positions(&[5, 5, 255, 3, 14, 11]), [2, 3, 5]);
    // 252 and above are skipped; 14 mod 12 is 2.
    assert_eq!(
        pick_positions(&[252, 253, 254, 255, 9, 21, 14, 1]),
        [1, 2, 9]
    );
    // Out of bytes: the lowest free places.
    assert_eq!(pick_positions(&[7, 255]), [0, 1, 7]);
    assert_eq!(pick_positions(&[]), [0, 1, 2]);
    for seed in 0..=255u8 {
        let p = pick_positions(&[seed, seed.wrapping_mul(5), seed.wrapping_add(77), 3, 4]);
        assert!(p[0] < p[1] && p[1] < p[2] && p[2] < 12);
    }
}

#[test]
fn sk_25_an_answer_is_the_first_four_letters_in_any_case() {
    assert!(answer_matches("abandon", "aban"));
    assert!(answer_matches("abandon", "ABAN"));
    assert!(answer_matches("abandon", "abandon"));
    assert!(!answer_matches("abandon", "aba"));
    assert!(!answer_matches("abandon", "abax"));
    // A word shorter than four letters is typed whole.
    assert!(answer_matches("zoo", "zoo"));
    assert!(!answer_matches("zoo", "zo"));
}

/// The App for a dice-alone run, standing at the check screen; returns the
/// words and the three positions asked.
fn to_check(a: &mut App<Fixed>) -> (Vec<String>, Vec<usize>) {
    a.key(Key::Char('2'));
    for d in rolls_of(TRIPLETS) {
        a.key(Key::Char(char::from(b'0' + d)));
    }
    a.key(Key::Enter);
    let words = shown_words(a);
    (words, pick_positions(&Fixed::new(1).0[0]).to_vec())
}

fn asked_position(a: &App<Fixed>) -> usize {
    let s = a.screen();
    assert_eq!(s.id, "check");
    let line = &s.lines[1];
    line.strip_prefix(words::SK_25_ASK)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

#[test]
fn sk_25_a_wrong_answer_shows_the_words_again_and_right_ones_go_on() {
    let mut a = app();
    let (w, _) = to_check(&mut a);
    // Dice alone takes no kernel bytes for the key: the first call is the positions.
    let expected = pick_positions(&Fixed::new(1).0[0]);
    for (i, pos) in expected.iter().enumerate() {
        assert_eq!(asked_position(&a), pos + 1);
        let word = if i == 1 {
            "zzzz".to_string()
        } else {
            w[*pos][..w[*pos].len().min(4)].to_string()
        };
        press(&mut a, &word);
        a.key(Key::Enter);
    }
    // The second was wrong: the words again, naming its position.
    let s = a.screen();
    assert_eq!(s.id, "words");
    assert_eq!(
        s.lines[0],
        format!("{} {}", words::SK_25_WRONG, expected[1] + 1)
    );
    assert_eq!(shown_words(&mut a), w);
    for pos in expected {
        press(
            &mut a,
            &w[pos][..w[pos].len().min(4)].to_uppercase().to_lowercase(),
        );
        a.key(Key::Enter);
    }
    assert_eq!(a.screen().id, "result");
}

#[test]
fn sk_26_the_outputs_equal_the_key_makers_for_the_same_words() {
    // The test seed of KM-14 and the 20 vectors of test/electrum.
    let dir = root().join("../../tools/keymaker/test/electrum");
    let expected = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    let mut n = 0;
    for line in expected.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (bits, words, fp, xpub, addr) = (f[0], &f[1..13], f[13], f[14], f[15]);
        let entropy = entropy16(&bits_to_bytes(bits));
        assert_eq!(words_of(&entropy).unwrap(), words, "{bits}");
        let p = public_of(words).unwrap();
        assert_eq!(p.fingerprint, fp);
        assert_eq!(p.address, addr);
        assert!(
            p.descriptor
                .contains(&format!("[{fp}/84h/0h/0h]{xpub}/<0;1>/*)#")),
            "{}",
            p.descriptor
        );
        // The descriptor passes the BIP380 check and reads back.
        assert_eq!(
            SingleKeyWallet::parse(&p.descriptor)
                .unwrap()
                .canonical_string(),
            p.descriptor
        );
        n += 1;
    }
    assert_eq!(n, 20);
}

#[test]
fn sk_26_the_test_seed_gives_its_fingerprint_and_first_address() {
    let words: Vec<&str> =
        "primary fetch primary fetch primary fetch primary fetch primary fetch primary fever"
            .split(' ')
            .collect();
    let p = public_of(&words).unwrap();
    assert_eq!(p.fingerprint, "818bc9b6");
    assert_eq!(p.address, "bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj");
}

#[test]
fn sk_26_the_screen_shows_the_text_and_a_qr_of_the_descriptor() {
    let mut a = app();
    a.key(Key::Char('3'));
    for w in "primary fetch primary fetch primary fetch primary fetch primary fetch primary fever"
        .split(' ')
    {
        press(&mut a, w);
        a.key(Key::Enter);
    }
    let s = a.screen();
    assert_eq!(s.id, "result");
    let t = text(&s);
    assert!(t.contains(&format!("{} 818bc9b6", words::SK_26_FINGERPRINT)));
    assert!(t.contains("bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj"));
    let desc = s
        .lines
        .iter()
        .find_map(|l| l.strip_prefix(&format!("{} ", words::SK_26_DESCRIPTOR)))
        .unwrap()
        .to_string();
    assert!(desc.starts_with("wpkh([818bc9b6/84h/0h/0h]xpub6CgURS1GrKZk"));
    a.key(Key::Enter);
    let q = a.screen();
    assert_eq!(q.id, "result-qr");
    assert_eq!(
        q.qr.as_ref(),
        Some(&openbmt_qr::encode_text(&desc, Ecc::M).unwrap())
    );
    // The picture, drawn by the program's own drawing, reads back.
    let (w, h) = (800, 600);
    let buf = render(&q, w, h);
    let luma: Vec<u8> = buf
        .iter()
        .map(|p| (p & 0xff) as u8 | if *p == 0xffffff { 0xff } else { 0 })
        .collect();
    let d = decode_luma(w, h, &luma).unwrap();
    assert_eq!(d.bytes, desc.as_bytes());
    assert_eq!(d.ecc, Ecc::M);
    a.key(Key::Enter);
    assert_eq!(a.screen().id, "result");
}

/// The 12-word vectors of the SeedSigner standard (docs/seed_qr/README.md):
/// words and the Compact SeedQR bitstream.
const SEEDSIGNER: &[(&str, &str)] = &[
    ("forum undo fragile fade shy sign arrest garment culture tube off merit", "01011011101111011001110101110001101010001110110001111001100100001000001100011010111111110011010110011101010000100110010101000101"),
    ("good battle boil exact add seed angle hurry success glad carbon whisper", "01100100011000100110100001100100001001110010000000110011100001011100001000110011011111011101100001001100010100001000100111111101"),
    ("approve fruit lens brass ring actual stool coin doll boss strong rate", "00001010110010111011101000000000100011011001101110100000000001011111010110011001011010110100000010100011010001110101110011011001"),
    ("dignity utility vacant shiver thought canoe feel multiply item youth actor coyote", "00111110000111100000101111000001111000110001111000001110010000110001010100110100100010110111011011011111111011000000101010011000"),
    ("corn voice scrap arrow original diamond trial property benefit choose junk lock", "00110000011111101010111100000101100001100101100111001010011110100111101000001101011000110001010100100101000010011110010101000001"),
    ("vocal tray giggle tool duck letter category pattern train magnet excite swamp", "11110101010111001111010110000111111100100101010000111101000000010000100100001101000010101110011100010000101111010011101101101101"),
];

/// A drawing of a code as gray levels, black on white, quiet zone 4.
fn gray_of(qr: &openbmt_qr::QrCode, scale: usize) -> (usize, usize, Vec<u8>) {
    let side = (qr.size() + 8) * scale;
    let mut luma = vec![255u8; side * side];
    for y in 0..qr.size() {
        for x in 0..qr.size() {
            if qr.get(x, y) {
                for dy in 0..scale {
                    for dx in 0..scale {
                        luma[((y + 4) * scale + dy) * side + (x + 4) * scale + dx] = 0;
                    }
                }
            }
        }
    }
    (side, side, luma)
}

#[test]
fn sk_27_compact_seedqr_equals_the_seedsigner_examples() {
    for (words, bits) in SEEDSIGNER {
        let w: Vec<&str> = words.split(' ').collect();
        let m = bip39::Mnemonic::parse_normalized(words).unwrap();
        let entropy = entropy16(&m.to_entropy());
        assert_eq!(entropy.to_vec(), bits_to_bytes(bits), "{words}");
        let qr = compact_seedqr(&entropy).unwrap();
        // 12 words: 16 bytes, byte mode, level L, 21 x 21.
        assert_eq!((qr.size(), qr.version(), qr.ecc()), (21, 1, Ecc::L));
        let (a, b, luma) = gray_of(&qr, 4);
        let d = decode_luma(a, b, &luma).unwrap();
        assert_eq!(d.bytes, entropy.to_vec(), "{words}");
        assert_eq!(words_of(&entropy).unwrap(), w);
    }
}

#[test]
fn sk_27_the_seedqr_screen_warns_and_shows_the_code() {
    let mut a = app();
    a.key(Key::Char('3'));
    for w in SEEDSIGNER[0].0.split(' ') {
        press(&mut a, w);
        a.key(Key::Enter);
    }
    a.key(Key::Char('s'));
    let s = a.screen();
    assert_eq!(s.id, "seedqr");
    assert!(text(&s).contains(words::SK_27_WARNING));
    let qr = s.qr.unwrap();
    let (w, h, luma) = gray_of(&qr, 4);
    assert_eq!(
        decode_luma(w, h, &luma).unwrap().bytes,
        bits_to_bytes(SEEDSIGNER[0].1)
    );
}

#[test]
fn sk_28_typed_words_are_checked_against_the_bip39_checksum() {
    let good: Vec<&str> = SEEDSIGNER[1].0.split(' ').collect();
    assert_eq!(
        check_words(&good).unwrap().to_vec(),
        bits_to_bytes(SEEDSIGNER[1].1)
    );
    // One word changed: a word of the list, the wrong checksum.
    let mut bad = good.clone();
    bad[11] = "abandon";
    assert_eq!(check_words(&bad), Err(TypedError::BadChecksum));
    bad[11] = "notaword";
    assert_eq!(check_words(&bad), Err(TypedError::BadWord));
    assert_eq!(check_words(&good[..11]), Err(TypedError::Count));
}

#[test]
fn sk_28_letters_make_a_word_by_its_first_letters_or_whole() {
    assert_eq!(complete_word("aban"), Some("abandon"));
    assert_eq!(complete_word("ABANDON"), Some("abandon"));
    assert_eq!(complete_word("zoo"), Some("zoo"));
    // A short word that starts others is itself when whole, else refused.
    assert_eq!(complete_word("ab"), None);
    assert_eq!(complete_word("zzzz"), None);
    assert_eq!(complete_word(""), None);
}

#[test]
fn sk_28_the_screen_names_a_bad_word_and_a_bad_checksum_and_allows_a_fix() {
    let mut a = app();
    a.key(Key::Char('3'));
    press(&mut a, "qqq");
    a.key(Key::Enter);
    assert!(text(&a.screen()).contains(words::SK_28_BAD_WORD));
    for _ in 0..3 {
        a.key(Key::Backspace);
    }
    let words12: Vec<&str> = SEEDSIGNER[0].0.split(' ').collect();
    for w in &words12[..11] {
        press(&mut a, w);
        a.key(Key::Enter);
    }
    // The wrong 12th word: the checksum fails, the word stays to correct.
    press(&mut a, "abandon");
    a.key(Key::Enter);
    let t = text(&a.screen());
    assert!(t.contains(words::SK_28_BAD_CHECKSUM), "{t}");
    assert!(t.contains("> abandon"));
    for _ in 0..7 {
        a.key(Key::Backspace);
    }
    press(&mut a, "merit");
    a.key(Key::Enter);
    assert_eq!(a.screen().id, "result");
}

#[test]
fn sk_29_every_screen_after_the_words_has_turn_off_and_it_clears_and_powers_off() {
    let mut a = app();
    a.key(Key::Char('3'));
    for w in SEEDSIGNER[2].0.split(' ') {
        press(&mut a, w);
        a.key(Key::Enter);
    }
    for key in [Key::Enter, Key::Enter, Key::Char('s'), Key::Enter] {
        a.key(key);
        let s = a.screen();
        assert!(
            text(&s).contains(&format!("Esc {}", words::SK_29_TURN_OFF)),
            "{}",
            s.id
        );
    }
    assert_eq!(a.key(Key::Escape), Outcome::TurnOff);
    // Nothing of the key is left: the app is back at the first screen with
    // no code.
    let s = a.screen();
    assert_eq!(s.id, "start");
    assert!(s.qr.is_none());
}

#[test]
fn sk_06_sk_12_sk_31_the_first_screen_shows_the_facts_and_the_interfaces() {
    let s = app().screen();
    assert_eq!(s.id, "start");
    let t = text(&s);
    assert!(t.contains(&format!("{} {}", words::SK_06_SECURE_BOOT, words::SK_06_ON)));
    // The firmware's other answers are named, never shown as a bare number.
    for (value, want) in [
        ("0", words::SK_06_OFF),
        ("?", words::SK_06_UNKNOWN),
        ("7", words::SK_06_UNKNOWN),
    ] {
        let mut f = facts();
        f.secure_boot = value.into();
        f.interfaces.clear();
        let t = text(&App::new(f, Fixed::new(1)).screen());
        assert!(
            t.contains(&format!("{} {want}", words::SK_06_SECURE_BOOT)),
            "{t}"
        );
        assert!(
            t.contains(&format!(
                "{} {}",
                words::SK_12_INTERFACES,
                words::SK_12_NONE
            )),
            "{t}"
        );
    }
    assert!(t.contains(&format!("{} integrity", words::SK_06_LOCKDOWN)));
    assert!(t.contains(&format!("{} lo", words::SK_12_INTERFACES)));
    assert!(t.contains(&format!("{} 0.1.0 test", words::SK_31_VERSION)));
    assert!(t.contains(&format!("{} {}", words::SK_31_INITRD_HASH, "ab".repeat(32))));
}

#[test]
fn sk_15_every_word_is_distinct_and_some_screen_shows_each() {
    let mut seen = String::new();
    let mut note = |a: &App<Fixed>| {
        seen.push_str(&text(&a.screen()));
        seen.push('\n');
    };
    let mut a = app();
    note(&a);
    a.key(Key::Down);
    a.key(Key::Up);
    a.key(Key::Char('1'));
    note(&a);
    a.key(Key::Escape);
    a.key(Key::Char('2'));
    note(&a);
    a.key(Key::Escape);
    // A whole dice-alone run with a wrong answer on the way.
    let (w, _) = to_check(&mut a);
    note(&a);
    let pos = pick_positions(&Fixed::new(1).0[1]);
    for p in pos {
        let _ = p;
        press(&mut a, "zzzz");
        a.key(Key::Enter);
    }
    note(&a);
    let _ = w;
    // Typed words, with both mistakes.
    let mut b = app();
    b.key(Key::Char('3'));
    press(&mut b, "qqq");
    b.key(Key::Enter);
    note(&b);
    for _ in 0..3 {
        b.key(Key::Backspace);
    }
    for w in SEEDSIGNER[0].0.split(' ').take(11) {
        press(&mut b, w);
        b.key(Key::Enter);
    }
    press(&mut b, "abandon");
    b.key(Key::Enter);
    note(&b);
    for _ in 0..7 {
        b.key(Key::Backspace);
    }
    press(&mut b, "merit");
    b.key(Key::Enter);
    note(&b);
    b.key(Key::Enter);
    note(&b);
    b.key(Key::Enter);
    b.key(Key::Char('s'));
    note(&b);
    // The first screen's other states (SK-06, SK-12).
    let mut f = facts();
    f.secure_boot = "0".into();
    f.interfaces.clear();
    note(&App::new(f.clone(), Fixed::new(1)));
    f.secure_boot = "?".into();
    note(&App::new(f, Fixed::new(1)));
    // The screens of the keys for the shop's wallets and of signing a payment.
    seen.push_str(&common::walk_new_screens());
    for name in words::ALL {
        assert!(seen.contains(name), "no screen shows {name}");
        assert!(
            words::ALL.iter().filter(|n| **n == *name).count() == 1,
            "{name} listed twice"
        );
    }
    // No placeholder remains: the words are written by the owner of the
    // rules, not by the code.
    let src = std::fs::read_to_string(root().join("src/words.rs")).unwrap();
    let tag = ["WORDS", "_NEEDED"].concat();
    assert!(!src.contains(&tag), "a placeholder remains in words.rs");
    // Every constant is named for its rule.
    for line in src
        .lines()
        .filter(|l| l.starts_with("pub const ") && !l.starts_with("pub const ALL"))
    {
        assert!(line.starts_with("pub const SK_"), "{line}");
    }
}

fn source_files() -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![root().join("src")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let t = std::fs::read_to_string(&p).unwrap();
                out.push((p, t));
            }
        }
    }
    out
}

#[test]
fn sk_12_the_source_has_no_network_call() {
    // The names are built from pieces so that this file would pass too.
    let banned = [
        ["sock", "et"].concat(),
        ["Tcp", "Stream"].concat(),
        ["Udp", "Socket"].concat(),
        ["Unix", "Stream"].concat(),
        ["Unix", "Listener"].concat(),
        ["Unix", "Datagram"].concat(),
        ["AF", "_INET"].concat(),
        ["AF", "_UNIX"].concat(),
        ["AF", "_PACKET"].concat(),
        ["AF", "_NETLINK"].concat(),
        ["net", "link"].concat(),
        ["SIOC", "SIF"].concat(),
        ["if", "config"].concat(),
        ["send", "to("].concat(),
        ["recv", "from("].concat(),
        ["std::", "net"].concat(),
        ["SYS_", "connect"].concat(),
        ["SYS_", "bind"].concat(),
    ];
    let files = source_files();
    assert!(files.len() >= 10);
    for (path, text) in &files {
        let lower = text.to_lowercase();
        for b in &banned {
            assert!(
                !lower.contains(&b.to_lowercase()),
                "{} holds {b}",
                path.display()
            );
        }
    }
    // The manifest pulls in no network package either.
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).unwrap();
    for dep in [
        "openbmt-net",
        "openbmt-link",
        "openbmt-courier",
        "tokio",
        "hyper",
        "reqwest",
    ] {
        assert!(!manifest.contains(dep), "{dep}");
    }
}

#[test]
fn sk_11_the_mounts_are_the_listed_ones_and_no_disk() {
    let types: Vec<&str> = MOUNTS.iter().map(|m| m.fstype).collect();
    assert_eq!(
        types,
        ["proc", "sysfs", "devtmpfs", "efivarfs", "securityfs"]
    );
    for m in MOUNTS {
        assert!(!NEVER.contains(&m.fstype));
    }
    // efivarfs and securityfs are mounted read only.
    assert!(MOUNTS.iter().filter(|m| m.read_only).count() == 2);
    // Nothing else in the source calls mount.
    for (path, text) in source_files() {
        if path.ends_with("sys.rs") {
            assert_eq!(text.matches("libc::mount(").count(), 1);
        } else {
            assert!(!text.contains("libc::mount("), "{}", path.display());
        }
    }
}

#[test]
fn sk_10_the_program_starts_no_other_program() {
    for (path, text) in source_files() {
        for banned in ["Command", "execv", "fork(", "posix_spawn", "system("] {
            assert!(!text.contains(banned), "{} holds {banned}", path.display());
        }
    }
}

#[test]
fn sk_13_the_program_writes_no_file() {
    for (path, text) in source_files() {
        for banned in [
            "fs::write",
            "File::create",
            "create(true)",
            "create_new",
            "remove_file",
            "rename(",
            "O_CREAT",
            "libc::unlink",
        ] {
            assert!(!text.contains(banned), "{} holds {banned}", path.display());
        }
    }
}

#[test]
fn sk_14_the_keys_come_from_input_events() {
    assert_eq!(key_from_code(28), Some(Key::Enter));
    assert_eq!(key_from_code(1), Some(Key::Escape));
    assert_eq!(key_from_code(14), Some(Key::Backspace));
    assert_eq!(key_from_code(2), Some(Key::Char('1')));
    assert_eq!(key_from_code(7), Some(Key::Char('6')));
    assert_eq!(key_from_code(11), Some(Key::Char('0')));
    assert_eq!(key_from_code(16), Some(Key::Char('q')));
    assert_eq!(key_from_code(31), Some(Key::Char('s')));
    assert_eq!(key_from_code(50), Some(Key::Char('m')));
    assert_eq!(key_from_code(79), Some(Key::Char('1')));
    assert_eq!(key_from_code(57), None);
    // All 26 letters are reachable.
    let letters: std::collections::BTreeSet<char> = (0..256u16)
        .filter_map(key_from_code)
        .filter_map(|k| if let Key::Char(c) = k { Some(c) } else { None })
        .filter(char::is_ascii_lowercase)
        .collect();
    assert_eq!(letters.len(), 26);
    let ev = |kind: u16, code: u16, value: i32| {
        let mut raw = [0u8; EVENT_SIZE];
        raw[16..18].copy_from_slice(&kind.to_ne_bytes());
        raw[18..20].copy_from_slice(&code.to_ne_bytes());
        raw[20..24].copy_from_slice(&value.to_ne_bytes());
        raw
    };
    assert_eq!(key_from_event(&ev(1, 28, 1)), Some(Key::Enter));
    // A release, a repeat and another kind of event give nothing.
    assert_eq!(key_from_event(&ev(1, 28, 0)), None);
    assert_eq!(key_from_event(&ev(1, 28, 2)), None);
    assert_eq!(key_from_event(&ev(4, 28, 1)), None);
}

#[test]
fn sk_14_long_lines_break_at_spaces_and_long_words_are_cut() {
    assert_eq!(
        wrap(
            "Compare the version with the release page before you go on.",
            20
        ),
        [
            "Compare the version",
            "with the release",
            "page before you go",
            "on."
        ]
    );
    assert_eq!(
        wrap("Fingerprint: 0123456789abcdef", 8),
        ["Fingerpr", "int:", "01234567", "89abcdef"]
    );
    assert_eq!(wrap("ab 0123456789", 4), ["ab", "0123", "4567", "89"]);
    assert_eq!(wrap("  2 Make", 20), ["  2 Make"]);
    assert_eq!(wrap("", 20), [""]);
    for line in wrap(&"word ".repeat(40), 13) {
        assert!(line.chars().count() <= 13, "{line}");
    }
}

#[test]
fn sk_14_the_screen_is_drawn_large_and_packed_in_the_depth_the_buffer_reports() {
    let mut a = app();
    let s = a.screen();
    let (w, h) = (800, 600);
    let buf = render(&s, w, h);
    assert_eq!(buf.len(), w * h);
    assert!(buf.iter().filter(|p| **p == 0x00ff_ffff).count() > 1000);
    assert_eq!(text_scale(800), 2);
    assert_eq!(text_scale(1920), 3);
    assert_eq!(text_scale(320), 1);
    // A QR screen draws white paper and black modules.
    a.key(Key::Char('3'));
    for w in SEEDSIGNER[0].0.split(' ') {
        press(&mut a, w);
        a.key(Key::Enter);
    }
    a.key(Key::Enter);
    let q = render(&a.screen(), w, h);
    assert!(q.iter().filter(|p| **p == 0).count() > 500);
    // Packing: 32 bits as BGRA, 16 bits as 5-6-5, 24 bits.
    let bgra = PixelFormat {
        bits_per_pixel: 32,
        red: Channel {
            offset: 16,
            length: 8,
        },
        green: Channel {
            offset: 8,
            length: 8,
        },
        blue: Channel {
            offset: 0,
            length: 8,
        },
    };
    assert_eq!(pack(0x00112233, &bgra), 0x00112233);
    let rgb565 = PixelFormat {
        bits_per_pixel: 16,
        red: Channel {
            offset: 11,
            length: 5,
        },
        green: Channel {
            offset: 5,
            length: 6,
        },
        blue: Channel {
            offset: 0,
            length: 5,
        },
    };
    assert_eq!(pack(0x00ffffff, &rgb565), 0xffff);
    assert_eq!(pack(0x00ff0000, &rgb565), 0xf800);
    assert_eq!(pack(0x0000ff00, &rgb565), 0x07e0);
    let mut row16 = [0u8; 4];
    write_row(&mut row16, &[0x00ff0000, 0x000000ff], &rgb565);
    assert_eq!(row16, [0x00, 0xf8, 0x1f, 0x00]);
    let rgb24 = PixelFormat {
        bits_per_pixel: 24,
        ..bgra
    };
    let mut row24 = [0u8; 6];
    write_row(&mut row24, &[0x00010203, 0x00040506], &rgb24);
    assert_eq!(row24, [3, 2, 1, 6, 5, 4]);
}

fn stick_file(name: &str) -> String {
    std::fs::read_to_string(root().join("../../stick").join(name)).unwrap()
}

#[test]
fn sk_04_the_modules_list_is_the_usb_keyboard_drivers_and_nothing_forbidden() {
    let text = stick_file("modules.list");
    let list = check_list(&text).unwrap();
    assert_eq!(
        list,
        [
            "kernel/drivers/hid/hid.ko.zst",
            "kernel/drivers/hid/hid-generic.ko.zst",
            "kernel/drivers/hid/usbhid/usbhid.ko.zst",
        ]
    );
    // The build refuses the same paths (stick/build.sh keeps its own copy of
    // the folders).
    let build = stick_file("build.sh");
    for f in FORBIDDEN {
        assert!(
            build.contains(&format!("{f}*")),
            "build.sh does not refuse {f}"
        );
    }
}

#[test]
fn sk_04_the_camera_list_is_the_uvc_driver_and_what_it_needs_and_nothing_else() {
    let text = stick_file("modules-camera.list");
    let list = check_camera_list(&text).unwrap();
    // The driver and, by `modinfo -F depends`, everything it needs.
    for needed in [
        "uvcvideo.ko.zst",
        "uvc.ko.zst",
        "videodev.ko.zst",
        "mc.ko.zst",
        "videobuf2-v4l2.ko.zst",
        "videobuf2-common.ko.zst",
        "videobuf2-vmalloc.ko.zst",
        "videobuf2-memops.ko.zst",
    ] {
        assert!(
            list.iter().any(|l| l.ends_with(&format!("/{needed}"))),
            "{needed}"
        );
    }
    assert_eq!(list.len(), 8, "{list:?}");
    // No module is in both lists, and the first list holds nothing of the
    // camera's folders.
    let first_text = stick_file("modules.list");
    let first = check_list(&first_text).unwrap();
    for m in &list {
        assert!(!first.contains(m), "{m}");
    }
    assert!(first
        .iter()
        .all(|m| !m.starts_with("kernel/drivers/media/")));
    // The build refuses the same camera folders.
    let build = stick_file("build.sh");
    for f in CAMERA_ALLOWED {
        let pattern = if f.ends_with('/') {
            format!("{f}*")
        } else {
            f.to_string()
        };
        assert!(build.contains(&pattern), "build.sh does not allow {f}");
    }
    // They are in initrd.img, apart from the first list's folder, so that
    // the loader's pass over /lib/modules never loads them.
    assert!(build.contains("/lib/camera-modules/"));
}

#[test]
fn sk_04_a_camera_list_with_any_other_module_is_refused() {
    for bad in [
        "kernel/drivers/media/usb/gspca/gspca_main.ko.zst",
        "kernel/drivers/media/dvb-core/dvb-core.ko.zst",
        "kernel/drivers/media/common/other.ko.zst",
        "kernel/drivers/net/ethernet/intel/e1000e/e1000e.ko.zst",
        "kernel/drivers/hid/hid.ko.zst",
        "kernel/drivers/usb/storage/usb-storage.ko.zst",
        "kernel/sound/core/snd.ko.zst",
        "kernel/drivers/media/usb/uvc/../../../net/x.ko.zst",
        "kernel/drivers/media/usb/uvc/uvcvideo.ko",
    ] {
        assert!(
            check_camera_list(&format!(
                "kernel/drivers/media/usb/uvc/uvcvideo.ko.zst\n{bad}\n"
            ))
            .is_err(),
            "{bad}"
        );
    }
    assert!(check_camera_list("").is_err());
    assert!(check_camera_list("# only a comment\n").is_err());
    // The camera modules are not allowed on the first list.
    assert!(check_list("kernel/drivers/media/usb/uvc/uvcvideo.ko.zst").is_err());
}

#[test]
fn sk_04_a_list_with_a_forbidden_module_is_refused() {
    for bad in [
        "kernel/drivers/net/ethernet/intel/e1000e/e1000e.ko.zst",
        "kernel/net/wireless/cfg80211.ko.zst",
        "kernel/drivers/bluetooth/btusb.ko.zst",
        "kernel/drivers/usb/storage/usb-storage.ko.zst",
        "kernel/drivers/scsi/sd_mod.ko.zst",
        "kernel/drivers/ata/libata.ko.zst",
        "kernel/drivers/nvme/host/nvme.ko.zst",
        "kernel/fs/ext4/ext4.ko.zst",
        "kernel/sound/core/snd.ko.zst",
        "kernel/drivers/media/usb/uvc/uvcvideo.ko.zst",
    ] {
        assert!(
            check_list(&format!("kernel/drivers/hid/hid.ko.zst\n{bad}\n")).is_err(),
            "{bad}"
        );
    }
    assert!(check_list("").is_err());
    assert!(check_list("# only a comment\n").is_err());
    assert!(check_list("kernel/drivers/hid/../../fs/x.ko.zst").is_err());
    assert!(check_list("/lib/modules/x.ko").is_err());
    assert!(check_list("kernel/drivers/hid/hid.ko").is_err());
}

#[test]
fn sk_02_the_inputs_lock_names_four_pinned_packages() {
    let text = stick_file("inputs.lock");
    let rows: Vec<Vec<&str>> = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split_whitespace().collect())
        .collect();
    let names: Vec<&str> = rows.iter().map(|r| r[0]).collect();
    assert_eq!(
        names,
        [
            "shim-signed",
            "grub-efi-amd64-signed",
            "linux-image-7.0.0-42-generic",
            "linux-modules-7.0.0-42-generic"
        ]
    );
    for r in &rows {
        assert_eq!(r.len(), 3, "{r:?}");
        assert_eq!(r[2].len(), 64, "{r:?}");
        assert!(
            r[2].bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "{r:?}"
        );
    }
    // The build refuses a package whose hash differs, and downloads by name
    // and version only.
    let build = stick_file("build.sh");
    assert!(build.contains("differs from inputs.lock"));
    assert!(build.contains("apt-get download \"$pkg=$ver\""));
}

#[test]
fn sk_03_the_command_line_is_one_fixed_line_and_the_kernel_enforces_signatures() {
    let text = stick_file("cmdline");
    assert_eq!(text.lines().count(), 1);
    assert!(text.ends_with('\n'));
    let words: Vec<&str> = text.split_whitespace().collect();
    assert!(words.contains(&"module.sig_enforce=1"));
    assert!(
        words.contains(&"retain_initrd"),
        "SK-31 reads the initrd from memory"
    );
    for w in &words {
        assert!(
            !w.starts_with("init=") && !w.starts_with("rdinit=") && !w.starts_with("root="),
            "{w}"
        );
    }
    // grub.cfg boots the kernel and initrd.img and nothing else, with no menu.
    let build = stick_file("build.sh");
    assert!(build.contains("linux /vmlinuz $cmdline"));
    assert!(build.contains("initrd /initrd.img"));
    assert!(!build.contains("menuentry"));
}

#[test]
fn sk_26_the_test_seeds_descriptor_has_the_key_makers_checksum() {
    let words: Vec<&str> =
        "primary fetch primary fetch primary fetch primary fetch primary fetch primary fever"
            .split(' ')
            .collect();
    let p = public_of(&words).unwrap();
    // tools/keymaker/test/core.test.js, KM-06.
    assert!(
        p.descriptor.ends_with("/<0;1>/*)#96yh9yhx"),
        "{}",
        p.descriptor
    );
}

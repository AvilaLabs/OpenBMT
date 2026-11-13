//! Tests of the keys for the shop's wallets (SK-32 to SK-34) and of signing a
//! payment (SK-50 to SK-58). They run on any computer; the whole boot is
//! `stick/test-qemu.sh`.

mod common;

use openbmt_qr::{decode_luma, QrCode};
use openbmt_stick::frames::{codes_of, texts_of, Collector, Feed, Form};

/// A tiny xorshift, so that the tests need no dependency and repeat.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// A QR code as a camera sees a phone's screen: the code at `module` pixels
/// a module on a light screen, turned by `degrees` about the frame's middle
/// and moved off centre, blurred by `blur` passes of a 3 x 3 box, with noise of
/// +-`noise` grey levels, on a 640 x 480 frame.
fn camera_frame(
    code: &QrCode,
    module: f32,
    degrees: f32,
    blur: usize,
    noise: f32,
    seed: u64,
) -> Vec<u8> {
    let (w, h) = (640usize, 480usize);
    let n = code.size() as f32;
    let quiet = 4.0;
    let half = (n + 2.0 * quiet) * module / 2.0;
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (cx, cy) = (w as f32 / 2.0 + 9.0, h as f32 / 2.0 - 7.0);
    let mut img = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            // Back from the frame to the screen's own coordinates.
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            let u = dx * cos + dy * sin;
            let v = -dx * sin + dy * cos;
            // Beyond the screen: a dark room.
            let value = if u.abs() > half || v.abs() > half {
                60.0
            } else {
                let mx = ((u + half) / module - quiet).floor();
                let my = ((v + half) / module - quiet).floor();
                let dark = mx >= 0.0
                    && my >= 0.0
                    && (mx as usize) < code.size()
                    && (my as usize) < code.size()
                    && code.get(mx as usize, my as usize);
                if dark {
                    35.0
                } else {
                    215.0
                }
            };
            img[y * w + x] = value;
        }
    }
    for _ in 0..blur {
        let src = img.clone();
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let mut sum = 0.0;
                for yy in y - 1..=y + 1 {
                    for xx in x - 1..=x + 1 {
                        sum += src[yy * w + xx];
                    }
                }
                img[y * w + x] = sum / 9.0;
            }
        }
    }
    let mut rng = Rng(seed.max(1));
    img.iter()
        .map(|v| {
            (v + (rng.unit() * 2.0 - 1.0) * noise)
                .clamp(0.0, 255.0)
                .round() as u8
        })
        .collect()
}

fn decode_text(luma: &[u8]) -> Option<String> {
    decode_luma(640, 480, luma)
        .ok()
        .and_then(|d| String::from_utf8(d.bytes).ok())
}

#[test]
fn sk_58_the_decoder_reads_the_owner_phones_frames_seen_by_a_camera() {
    let psbt = common::big_payment(6).serialize();
    let mut total = 0;
    let mut read = 0;
    let mut failed: Vec<String> = Vec::new();
    for form in [Form::Ur, Form::Bbqr] {
        let texts = texts_of(form, &psbt).unwrap();
        let codes = codes_of(form, &psbt).unwrap();
        let size = codes[0].size();
        println!(
            "{form:?}: {} frames, {} modules a side, version {}",
            codes.len(),
            size,
            codes[0].version()
        );
        for (module, degrees, blur, noise) in [
            (6.0f32, 0.0f32, 0usize, 0.0f32),
            (5.0, 0.0, 0, 0.0),
            (6.0, 3.0, 0, 0.0),
            (6.0, -4.0, 1, 10.0),
            (6.0, 5.0, 1, 12.0),
            (5.0, 7.0, 1, 12.0),
            (5.0, -8.0, 2, 12.0),
            (4.0, 2.0, 1, 8.0),
            (5.0, 12.0, 1, 10.0),
            (5.0, 20.0, 1, 5.0),
        ] {
            let mut ok = 0;
            let n = texts.len().min(4);
            for (i, (text, code)) in texts.iter().zip(&codes).take(n).enumerate() {
                total += 1;
                let frame = camera_frame(code, module, degrees, blur, noise, 7 + i as u64);
                if decode_text(&frame).as_deref() == Some(text.as_str()) {
                    ok += 1;
                    read += 1;
                } else {
                    failed.push(format!(
                        "{form:?} module {module} turn {degrees} blur {blur} noise {noise}"
                    ));
                }
            }
            println!(
                "  module {module} px, turned {degrees:>4} deg, blur {blur}, noise +-{noise}: {ok}/{n}"
            );
        }
    }
    println!("{read}/{total} frames read; failed: {failed:?}");
    // The reader copes with the rotation (to 20 degrees here, and much more
    // at some sizes), the blur, the sizes and noise up to +-12 grey levels;
    // above +-14 it gives up (printed above, not asserted).
    assert_eq!(read, total, "{read}/{total}: {failed:?}");
}

#[test]
fn sk_58_every_part_of_a_payment_gathers_from_camera_frames() {
    // Whole runs: every frame of the animation seen as a camera sees it,
    // in a shuffled order with repeats, gathered into the same bytes.
    let psbt = common::big_payment(6).serialize();
    for form in [Form::Ur, Form::Bbqr] {
        let codes = codes_of(form, &psbt).unwrap();
        let mut c = Collector::new();
        let mut done = None;
        let n = codes.len();
        for round in 0..3 {
            for k in 0..n {
                let i = (k * 5 + round) % n;
                let frame = camera_frame(
                    &codes[i],
                    6.0,
                    3.0 - round as f32 * 2.0,
                    1,
                    12.0,
                    100 + i as u64,
                );
                if let Some(t) = decode_text(&frame) {
                    if let Feed::Done { form: f, psbt } = c.feed(&t) {
                        done = Some((f, psbt));
                    }
                }
                if done.is_some() {
                    break;
                }
            }
            if done.is_some() {
                break;
            }
        }
        let (f, got) = done.unwrap_or_else(|| panic!("{form:?} did not gather"));
        assert_eq!(f, form);
        assert_eq!(got, psbt);
    }
}

// ---- SK-32 to SK-34: the keys for the two everyday wallets ---------------

use std::str::FromStr;

use bitcoin::bip32::Xpriv;
use openbmt_stick::app::Outcome;
use openbmt_stick::bip85::{
    entropy_at, float_entropy, float_path, lightning_child, lightning_hex, lightning_node_id_hex,
    lightning_path,
};
use openbmt_stick::keys::Key;
use openbmt_stick::outputs::{master_of, words_of};
use openbmt_stick::seedqr::compact_seedqr;
use openbmt_stick::sign::{btc, hundredths, prepare, Refusal};
use openbmt_stick::words;

use common::{app, press, scanned, text, type_words, WORDS};

/// The BIP-85 specification's test key.
const SPEC_MASTER: &str = "xprv9s21ZrQH143K2LBWUUQRFXhucrQqBpKdRRxNVq2zBqsx8HVqFk2uYo8kmbaLLHRdqtQpUm98uKfu3vca1LqdGhUtyoFnCNkfmXRyPXLjbKb";
const SPEC_BIP39_12: &str =
    "girl mad pet galaxy egg matter matrix prison refuse sense ordinary nose";
const SPEC_HEX_64: &str = "492db4698cf3b73a5a24998aa3e9d7fa96275d85724a91e71aa2d645442f878555d078fd1f1f67e368976f04137b1f7a0d19232136ca50c44614af72b5582a5c";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn luma_of(qr: &QrCode, scale: usize) -> (usize, usize, Vec<u8>) {
    let n = qr.size() + 8;
    let side = n * scale;
    let mut out = vec![255u8; side * side];
    for y in 0..qr.size() {
        for x in 0..qr.size() {
            if qr.get(x, y) {
                for dy in 0..scale {
                    for dx in 0..scale {
                        out[((y + 4) * scale + dy) * side + (x + 4) * scale + dx] = 0;
                    }
                }
            }
        }
    }
    (side, side, out)
}

#[test]
fn sk_34_the_children_equal_the_bip85_specification_vectors() {
    let master = Xpriv::from_str(SPEC_MASTER).unwrap();
    let e = entropy_at(&master, &float_path(0)).unwrap();
    let m = bip39::Mnemonic::from_entropy(&e[..16]).unwrap();
    assert_eq!(m.to_string(), SPEC_BIP39_12);
    // The HEX application's path is the stick's own path of Shop Lightning.
    assert_eq!(lightning_path(), "m/83696968'/128169'/64'/0'");
    assert_eq!(
        hex(&entropy_at(&master, &lightning_path()).unwrap()),
        SPEC_HEX_64
    );
}

#[test]
fn sk_32_the_paths_are_the_rules_paths() {
    assert_eq!(float_path(0), "m/83696968'/39'/0'/12'/0'");
    assert_eq!(float_path(99), "m/83696968'/39'/0'/12'/99'");
}

#[test]
fn sk_32_the_float_at_index_one_differs_and_equals_the_derivation_at_one() {
    let master = master_of(&WORDS).unwrap();
    let zero = float_entropy(&WORDS, 0).unwrap();
    let one = float_entropy(&WORDS, 1).unwrap();
    assert_ne!(zero, one);
    assert_eq!(
        zero[..],
        entropy_at(&master, "m/83696968'/39'/0'/12'/0'").unwrap()[..16]
    );
    assert_eq!(
        one[..],
        entropy_at(&master, "m/83696968'/39'/0'/12'/1'").unwrap()[..16]
    );
    // 0 to 99 only.
    assert!(float_entropy(&WORDS, 99).is_ok());
    assert!(float_entropy(&WORDS, 100).is_err());
}

#[test]
fn sk_32_lightning_is_128_lower_case_hex_characters_of_the_hex_child() {
    let master = master_of(&WORDS).unwrap();
    let h = lightning_hex(&WORDS).unwrap();
    assert_eq!(h.len(), 128);
    assert!(h
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert_eq!(h, hex(&entropy_at(&master, &lightning_path()).unwrap()));
}

// ---- SK-35: the Lightning node's ID --------------------------------------

/// The app at the node ID screen of the test words.
fn node_id_app() -> openbmt_stick::app::App<common::Zero> {
    let mut a = result_app();
    a.key(Key::Char('l'));
    assert_eq!(a.screen().id, "lightning");
    a.key(Key::Char('n'));
    a
}

#[test]
fn sk_35_the_screen_follows_the_lightning_screen_and_enter_returns() {
    let mut a = node_id_app();
    assert_eq!(a.screen().id, "node-id");
    a.key(Key::Enter);
    assert_eq!(a.screen().id, "result");
    // N opens it from the Lightning screen only.
    a.key(Key::Char('n'));
    assert_eq!(a.screen().id, "result");
    a.key(Key::Char('f'));
    a.key(Key::Char('n'));
    assert_eq!(a.screen().id, "float");
    // The Lightning screen lists the key.
    let mut b = result_app();
    b.key(Key::Char('l'));
    assert!(text(&b).contains(&format!("N {}", words::SK_35_MENU_NODE_ID)));
}

#[test]
fn sk_35_the_qr_decodes_to_the_cores_id_in_66_lower_case_hex_characters() {
    let a = node_id_app();
    let screen = a.screen();
    let qr = screen.qr.as_ref().expect("a code on the screen");
    let (w, h, luma) = luma_of(qr, 6);
    let got = String::from_utf8(decode_luma(w, h, &luma).unwrap().bytes).unwrap();
    let mut seed = lightning_child(&WORDS).unwrap();
    let want = hex(&openbmt_core::shop_lightning::node_id(&seed));
    seed.fill(0);
    assert_eq!(got.len(), 66);
    assert_eq!(got, want);
    assert_eq!(got, lightning_node_id_hex(&WORDS).unwrap());
    assert!(got
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    // The ID is a point of the curve, not a key of the 64 bytes' own.
    assert!(got.starts_with("02") || got.starts_with("03"));
    assert_ne!(got, lightning_hex(&WORDS).unwrap()[..66]);
}

#[test]
fn sk_35_the_first_and_last_8_characters_are_lines_of_their_own() {
    let a = node_id_app();
    let screen = a.screen();
    let want = lightning_node_id_hex(&WORDS).unwrap();
    assert!(screen
        .lines
        .iter()
        .any(|l| l == &format!("{} {}", words::SK_35_FIRST, &want[..8])));
    assert!(screen
        .lines
        .iter()
        .any(|l| l == &format!("{} {}", words::SK_35_LAST, &want[58..])));
    // The whole ID is not text on the screen: the code carries it.
    assert!(!text(&a).contains(&want));
}

#[test]
fn sk_35_turning_off_clears_the_screen_and_its_code() {
    let mut a = node_id_app();
    a.key(Key::Escape);
    assert!(!a.holds_anything());
}

#[test]
fn sk_35_the_placeholders_are_the_ones_listed() {
    // Every new word is a placeholder until the owner of the rules writes
    // it; this list is that to-do.
    let want = [
        "WORDS_NEEDED_SK35_MENU_NODE_ID",
        "WORDS_NEEDED_SK35_PUBLIC",
        "WORDS_NEEDED_SK35_SCAN",
        "WORDS_NEEDED_SK35_FIRST",
        "WORDS_NEEDED_SK35_LAST",
    ];
    assert_eq!(words::placeholders(), want);
    assert!(words::is_placeholder(want[0]));
    assert!(!words::is_placeholder(words::SK_33_LIGHTNING_WHAT));
}

/// The app standing at the public result of the test words.
fn result_app() -> openbmt_stick::app::App<common::Zero> {
    let mut a = app();
    a.key(Key::Char('3'));
    type_words(&mut a, &WORDS);
    assert_eq!(a.screen().id, "result");
    a
}

#[test]
fn sk_32_f_and_l_open_the_two_screens_from_both_views_and_enter_returns() {
    for to_qr_first in [false, true] {
        let mut a = result_app();
        if to_qr_first {
            a.key(Key::Enter);
            assert_eq!(a.screen().id, "result-qr");
        }
        a.key(Key::Char('f'));
        assert_eq!(a.screen().id, "float");
        a.key(Key::Enter);
        assert_eq!(a.screen().id, "result");
        a.key(Key::Char('l'));
        assert_eq!(a.screen().id, "lightning");
        a.key(Key::Enter);
        assert_eq!(a.screen().id, "result");
    }
    // Both open from the text view's own list of keys.
    let t = text(&result_app());
    assert!(t.contains(&format!("F {}", words::SK_32_MENU_FLOAT)));
    assert!(t.contains(&format!("L {}", words::SK_32_MENU_LIGHTNING)));
}

#[test]
fn sk_32_the_float_screen_shows_the_index_and_the_seedqr_of_that_index() {
    let mut a = result_app();
    a.key(Key::Char('f'));
    let label = words::SK_32_FLOAT_INDEX;
    let qr_at = |i: u32| {
        compact_seedqr(&float_entropy(&WORDS, i).unwrap())
            .unwrap()
            .clone()
    };
    let same = |a: &openbmt_stick::app::App<common::Zero>, i: u32| {
        let s = a.screen();
        assert!(
            s.lines.iter().any(|l| *l == format!("{label} {i}")),
            "{:?}",
            s.lines
        );
        let want = qr_at(i);
        let got = s.qr.unwrap();
        assert_eq!(got.size(), want.size());
        for y in 0..got.size() {
            for x in 0..got.size() {
                assert_eq!(got.get(x, y), want.get(x, y), "index {i}");
            }
        }
    };
    same(&a, 0);
    a.key(Key::Char('+'));
    same(&a, 1);
    a.key(Key::Char('+'));
    a.key(Key::Char('-'));
    same(&a, 1);
    a.key(Key::Char('-'));
    a.key(Key::Char('-'));
    same(&a, 0);
    // 0 is the least, 99 the most.
    for _ in 0..150 {
        a.key(Key::Char('+'));
    }
    same(&a, 99);
    // The index is not kept across a new result.
    a.key(Key::Enter);
    a.key(Key::Char('f'));
    same(&a, 99);
    // The index 0 child is the specification's way of making a BIP39 child.
    let master = master_of(&WORDS).unwrap();
    let e = entropy_at(&master, "m/83696968'/39'/0'/12'/0'").unwrap();
    assert_eq!(float_entropy(&WORDS, 0).unwrap()[..], e[..16]);
}

#[test]
fn sk_33_each_screen_says_what_the_code_is_what_it_spends_how_often_it_is_scanned_and_no_photo() {
    let mut a = result_app();
    a.key(Key::Char('f'));
    let t = text(&a);
    for w in [
        words::SK_33_FLOAT_WHAT,
        words::SK_33_FLOAT_SPENDS,
        words::SK_33_FLOAT_SCAN,
        words::SK_33_FLOAT_NO_PHOTO,
    ] {
        assert!(t.contains(w), "{w}");
    }
    a.key(Key::Enter);
    a.key(Key::Char('l'));
    let t = text(&a);
    for w in [
        words::SK_33_LIGHTNING_WHAT,
        words::SK_33_LIGHTNING_SPENDS,
        words::SK_33_LIGHTNING_SCAN,
        words::SK_33_LIGHTNING_NO_PHOTO,
    ] {
        assert!(t.contains(w), "{w}");
    }
}

#[test]
fn sk_34_the_screens_codes_decode_to_the_same_bytes() {
    let mut a = result_app();
    a.key(Key::Char('f'));
    a.key(Key::Char('+'));
    let qr = a.screen().qr.unwrap();
    let (w, h, luma) = luma_of(&qr, 6);
    let d = decode_luma(w, h, &luma).unwrap();
    assert_eq!(d.bytes, float_entropy(&WORDS, 1).unwrap().to_vec());
    // That is a SeedQR of 12 words: the words of the child.
    let m = bip39::Mnemonic::from_entropy(&d.bytes).unwrap();
    assert_eq!(m.word_count(), 12);
    a.key(Key::Enter);
    a.key(Key::Char('l'));
    let qr = a.screen().qr.unwrap();
    let (w, h, luma) = luma_of(&qr, 6);
    let d = decode_luma(w, h, &luma).unwrap();
    assert_eq!(d.bytes, lightning_hex(&WORDS).unwrap().into_bytes());
    // The specification's own key gives the specification's child through the
    // same QR route.
    let master = Xpriv::from_str(SPEC_MASTER).unwrap();
    let spec = hex(&entropy_at(&master, &lightning_path()).unwrap());
    let q = openbmt_qr::encode_text(&spec, openbmt_qr::Ecc::M).unwrap();
    let (w, h, luma) = luma_of(&q, 6);
    assert_eq!(
        decode_luma(w, h, &luma).unwrap().bytes,
        SPEC_HEX_64.as_bytes()
    );
    let _ = words_of;
}

// ---- SK-50, SK-52: the choice and the camera -------------------------------

#[test]
fn sk_50_the_fourth_choice_loads_the_camera_and_no_other_does() {
    for key in ['1', '2', '3'] {
        let mut a = app();
        assert_eq!(a.key(Key::Char(key)), Outcome::Continue, "{key}");
    }
    let mut a = app();
    assert_eq!(a.key(Key::Char('4')), Outcome::LoadCamera);
    assert_eq!(a.screen().id, "sign-scan");
    // Down three times and Enter is the same choice.
    let mut b = app();
    for _ in 0..3 {
        b.key(Key::Down);
    }
    assert!(text(&b).contains(&format!("> 4 {}", words::SK_50_MENU_SIGN)));
    assert_eq!(b.key(Key::Enter), Outcome::LoadCamera);
    // Esc on the reading screen goes back to the first screen.
    b.key(Key::Escape);
    assert_eq!(b.screen().id, "start");
}

#[test]
fn sk_52_no_camera_in_the_set_time_says_so_and_the_owner_can_turn_off() {
    let mut a = app();
    a.key(Key::Char('4'));
    assert!(!a.tick(14_000));
    assert_eq!(a.screen().id, "sign-scan");
    assert!(a.tick(1_000));
    let s = a.screen();
    assert_eq!(s.id, "sign-nocamera");
    assert!(text(&a).contains(words::SK_52_NO_CAMERA));
    assert_eq!(a.key(Key::Escape), Outcome::TurnOff);
    // A camera that is open stops the clock.
    let mut b = app();
    b.key(Key::Char('4'));
    b.camera_found();
    b.tick(60_000);
    assert_eq!(b.screen().id, "sign-scan");
}

#[test]
fn sk_52_the_screen_shows_the_parts_read_out_of_the_parts_needed() {
    let psbt = common::big_payment(6).serialize();
    let texts = texts_of(Form::Ur, &psbt).unwrap();
    let mut a = app();
    a.key(Key::Char('4'));
    a.camera_found();
    let label = words::SK_52_PARTS;
    assert!(text(&a).contains(&format!("{label} 0/0")));
    a.frame_text(&texts[0]);
    let needed = texts.len() / 2;
    assert!(
        text(&a).contains(&format!("{label} 1/{needed}")),
        "{}",
        text(&a)
    );
    // A repeat adds nothing; another part adds one.
    assert!(!a.frame_text(&texts[0]));
    a.frame_text(&texts[1]);
    assert!(text(&a).contains(&format!("{label} 2/{needed}")));
    // A BBQr counts its own.
    let b = texts_of(Form::Bbqr, &psbt).unwrap();
    let mut c = app();
    c.key(Key::Char('4'));
    c.frame_text(&b[2]);
    assert!(text(&c).contains(&format!("{label} 1/{}", b.len())));
    // A text of no payment is no part.
    assert!(!c.frame_text("https://example.com"));
}

#[test]
fn sk_52_ur_and_bbqr_gather_in_any_order_with_repeats_and_other_codes_between() {
    let psbt = common::big_payment(6).serialize();
    for form in [Form::Ur, Form::Bbqr] {
        let texts = texts_of(form, &psbt).unwrap();
        let mut c = Collector::new();
        let mut done = None;
        for t in texts.iter().rev().chain(texts.iter()) {
            assert!(matches!(c.feed("not a part"), Feed::Progress { .. }));
            // A part of the other form is ignored once the first is set.
            let other = match form {
                Form::Ur => "B$2PA0100ABCDEFGH",
                Form::Bbqr => "ur:crypto-psbt/1-2/lpadaxcfax",
            };
            c.feed(other);
            if let Feed::Done { form: f, psbt: p } = c.feed(t) {
                done = Some((f, p));
                break;
            }
        }
        assert_eq!(done, Some((form, psbt.clone())), "{form:?}");
    }
}

// ---- SK-53 to SK-56: judging, showing and signing ---------------------------

use bitcoin::psbt::Psbt;
use bitcoin::{Address, Network};
use openbmt_core::wallet::Keychain;
use openbmt_spend::{check_finished, finalize, take_wallet_signature};

fn expected_refusal(name: &str) -> Refusal {
    match name {
        "NotAPsbt" => Refusal::NotAPsbt,
        "NotMainnet" => Refusal::NotMainnet,
        "NoPrevout" => Refusal::NoPrevout(1),
        "PrevoutMismatch" => Refusal::PrevoutMismatch(0),
        "NoPath" => Refusal::NoPath(0),
        "OtherWallet" => Refusal::OtherWallet(0),
        "PathNotWallet" => Refusal::PathNotWallet(0),
        "KeyMismatch" => Refusal::KeyMismatch(0),
        "SighashNotAll" => Refusal::SighashNotAll(1),
        "OutputsExceedInputs" => Refusal::OutputsExceedInputs,
        other => panic!("{other}"),
    }
}

#[test]
fn sk_54_the_good_payment_passes() {
    assert!(prepare(&common::payment().serialize(), &common::master()).is_ok());
}

#[test]
fn sk_54_each_refusal_names_why_one_by_one() {
    let master = common::master();
    let cases = common::refused_payments();
    assert_eq!(cases.len(), 10);
    for (name, bytes) in cases {
        let got = prepare(&bytes, &master).map(|_| ()).unwrap_err();
        assert_eq!(got, expected_refusal(name), "{name}");
    }
}

#[test]
fn sk_54_a_version_one_file_and_a_file_without_outputs_are_refused() {
    let master = common::master();
    let mut p = common::payment();
    p.version = 1;
    assert_eq!(
        prepare(&p.serialize(), &master).unwrap_err(),
        Refusal::NotAPsbt
    );
    let mut p = common::payment();
    p.unsigned_tx.output.clear();
    p.outputs.clear();
    assert_eq!(
        prepare(&p.serialize(), &master).unwrap_err(),
        Refusal::NotAPsbt
    );
}

#[test]
fn sk_54_a_key_of_the_test_coin_type_is_not_real_bitcoin() {
    let master = common::master();
    let mut p = common::payment();
    for (_, path) in p.inputs[0].bip32_derivation.values_mut() {
        *path = bitcoin::bip32::DerivationPath::from_str("m/84'/1'/0'/0/2").unwrap();
    }
    assert_eq!(
        prepare(&p.serialize(), &master).unwrap_err(),
        Refusal::NotMainnet
    );
}

#[test]
fn sk_53_a_payment_of_another_wallet_is_refused_before_anything_is_shown() {
    let mut a = scanned(&common::payment().serialize(), Form::Ur);
    let other: Vec<&str> = std::iter::repeat_n("abandon", 11)
        .chain(["about"])
        .collect();
    type_words(&mut a, &other);
    let s = a.screen();
    assert_eq!(s.id, "sign-refused");
    let t = text(&a);
    assert!(t.contains(words::SK_54_OTHER_WALLET), "{t}");
    assert!(t.contains(&format!("{} 1", words::SK_54_INPUT)));
    // No output, no amount.
    assert!(!t.contains("bc1"));
    assert!(!t.contains(words::SK_55_TITLE));
    // The words are not kept after a refusal; Esc turns off.
    assert_eq!(a.key(Key::Escape), Outcome::TurnOff);
    assert!(!a.holds_anything());
}

#[test]
fn sk_54_the_screen_names_each_refusal_and_its_input() {
    let want = [
        words::SK_52_BAD_FILE,
        words::SK_54_NOT_MAINNET,
        words::SK_54_NO_PREVOUT,
        words::SK_54_PREVOUT_MISMATCH,
        words::SK_54_NO_PATH,
        words::SK_54_OTHER_WALLET,
        words::SK_54_PATH_NOT_WALLET,
        words::SK_54_KEY_MISMATCH,
        words::SK_54_NOT_SIGHASH_ALL,
        words::SK_54_OUTPUTS_EXCEED,
    ];
    for ((name, bytes), reason) in common::refused_payments().into_iter().zip(want) {
        let mut a = scanned(&bytes, Form::Ur);
        type_words(&mut a, &WORDS);
        assert_eq!(a.screen().id, "sign-refused", "{name}");
        let t = text(&a);
        assert!(t.contains(reason), "{name}: {t}");
        assert!(t.contains(words::SK_54_REFUSED));
    }
}

fn parsed(bytes: &[u8]) -> Psbt {
    Psbt::deserialize(bytes).unwrap()
}

#[test]
fn sk_55_own_outputs_are_marked_only_when_derived_from_the_words() {
    let master = common::master();
    let p = common::payment();
    let r = prepare(&p.serialize(), &master).unwrap();
    let r = r.review();
    // The first output pays an outside script, the second is change at
    // change index 7 of this wallet.
    assert_eq!(r.outputs.len(), 2);
    let addr = |s: &bitcoin::ScriptBuf| {
        Address::from_script(s, Network::Bitcoin)
            .unwrap()
            .to_string()
    };
    assert_eq!(r.outputs[0].address, addr(&common::external(1)));
    assert!(!r.outputs[0].own);
    assert_eq!(
        r.outputs[1].address,
        addr(&common::script_at(Keychain::Change, 7))
    );
    assert!(r.outputs[1].own);
    assert_eq!(r.sent_away, 500_000);
    assert_eq!(r.outputs[0].sats, 500_000);
}

#[test]
fn sk_55_a_claim_in_the_file_is_not_enough_to_be_own() {
    let master = common::master();
    let mut p = common::payment();
    // The outside output claims to be the wallet's change: the file carries
    // the wallet's own key path for it.
    let own_hint = p.outputs[1].bip32_derivation.clone();
    assert!(!own_hint.is_empty());
    p.outputs[0].bip32_derivation = own_hint;
    let r = prepare(&p.serialize(), &master).unwrap();
    assert!(!r.review().outputs[0].own);
    assert_eq!(r.review().sent_away, 500_000);
    // And an own output stripped of every claim is still own.
    let mut q = common::payment();
    q.outputs[1] = Default::default();
    let r = prepare(&q.serialize(), &master).unwrap();
    assert!(r.review().outputs[1].own);
}

#[test]
fn sk_55_own_means_the_two_chains_up_to_the_stated_bound() {
    use openbmt_stick::sign::OWN_INDEX_BOUND;
    let master = common::master();
    for (chain, index, own) in [
        (Keychain::Receive, 0, true),
        (Keychain::Change, 0, true),
        (Keychain::Receive, OWN_INDEX_BOUND - 1, true),
        (Keychain::Change, OWN_INDEX_BOUND - 1, true),
        (Keychain::Receive, OWN_INDEX_BOUND, false),
    ] {
        // The planner refuses a payment to the wallet's own script, so the
        // output's script is put in the file by hand.
        let mut p = common::payment_of(
            &[common::coin(1, Keychain::Receive, 0, 300_000)],
            vec![(common::external(1), 100_000)],
            5,
        );
        p.unsigned_tx.output[0].script_pubkey = common::script_at(chain, index);
        let r = prepare(&p.serialize(), &master).unwrap();
        assert_eq!(r.review().outputs[0].own, own, "{chain:?} {index}");
        assert_eq!(r.review().sent_away, if own { 0 } else { 100_000 });
    }
}

#[test]
fn sk_55_the_totals_the_fee_and_the_rate_are_worked_out_from_the_file() {
    let master = common::master();
    let p = common::payment();
    let prepared = prepare(&p.serialize(), &master).unwrap();
    let r = prepared.review();
    let total_in = 300_000 + 400_000;
    let out: u64 = p.unsigned_tx.output.iter().map(|o| o.value.to_sat()).sum();
    assert_eq!(r.fee, total_in - out);
    // The estimate is the signed size, never below the real one and within
    // two percent of it.
    let signed = parsed(&prepared.sign(&master).unwrap());
    let both = take_wallet_signature(&common::wallet(), &p, &signed).unwrap();
    let tx = finalize(&common::wallet(), &both).unwrap();
    let real = tx.vsize() as u64;
    assert!(
        r.vsize >= real && r.vsize * 100 <= real * 102,
        "{} vs {real}",
        r.vsize
    );
    assert_eq!(r.fee_rate_hundredths(), r.fee * 100 / r.vsize);
    assert_eq!(btc(150_000_000), "1.50000000");
    assert_eq!(btc(1), "0.00000001");
    assert_eq!(hundredths(123), "1.23");
}

#[test]
fn sk_55_the_review_screen_lists_every_output_the_total_the_fee_and_the_rate() {
    let mut a = scanned(&common::payment().serialize(), Form::Ur);
    type_words(&mut a, &WORDS);
    assert_eq!(a.screen().id, "sign-review");
    let t = text(&a);
    let w = |n: &str| n.to_string();
    let ext = Address::from_script(&common::external(1), Network::Bitcoin)
        .unwrap()
        .to_string();
    assert!(t.contains(&format!("1 {ext}")), "{t}");
    assert!(t.contains(&format!("{} {}", btc(500_000), words::SK_55_UNIT_BTC)));
    assert!(t.contains(&format!("{} {}", words::SK_55_SENT, btc(500_000))));
    assert!(t.contains(&w(words::SK_55_FEE)));
    assert!(t.contains(&w(words::SK_55_FEE_RATE)));
    assert!(t.contains(&format!("Enter {}", words::SK_55_SIGN)));
    assert!(t.contains(&format!("Esc {}", words::SK_55_REFUSE)));
    // Exactly one output is marked own.
    assert_eq!(t.matches(words::SK_55_OWN).count(), 1);
    // Esc refuses and nothing is signed.
    assert_eq!(a.key(Key::Escape), Outcome::Continue);
    assert_eq!(a.screen().id, "start");
    assert!(!a.holds_anything());
}

#[test]
fn sk_55_more_than_three_outputs_take_pages() {
    let mut a = scanned(&common::many_outputs_payment().serialize(), Form::Ur);
    type_words(&mut a, &WORDS);
    let first = text(&a);
    assert!(first.contains(&format!("{} 1/2", words::SK_55_PAGE)));
    let ext = |n| {
        Address::from_script(&common::external(n), Network::Bitcoin)
            .unwrap()
            .to_string()
    };
    assert!(first.contains(&ext(1)) && first.contains(&ext(3)) && !first.contains(&ext(4)));
    a.key(Key::Down);
    let second = text(&a);
    assert!(second.contains(&ext(4)) && second.contains(&ext(5)) && !second.contains(&ext(1)));
    // The totals are on every page.
    assert!(second.contains(words::SK_55_SENT));
    a.key(Key::Up);
    assert_eq!(text(&a), first);
}

#[test]
fn sk_56_the_signatures_are_valid_and_nothing_else_of_the_file_changes() {
    let master = common::master();
    for p in [
        common::payment(),
        common::big_payment(5),
        common::many_outputs_payment(),
    ] {
        let prepared = prepare(&p.serialize(), &master).unwrap();
        let out = parsed(&prepared.sign(&master).unwrap());
        // The spend path's own check: every signature verifies against the
        // BIP 143 hash of its input and is the input's own key's.
        let both = take_wallet_signature(&common::wallet(), &p, &out).unwrap();
        // The transaction finishes into a valid, checked transaction.
        let tx = finalize(&common::wallet(), &both).unwrap();
        check_finished(&common::wallet(), &p, &tx).unwrap();
        // One signature on each input, SIGHASH_ALL, nothing finalized.
        for input in &out.inputs {
            assert_eq!(input.partial_sigs.len(), 1);
            let sig = input.partial_sigs.values().next().unwrap();
            assert_eq!(sig.sighash_type, bitcoin::sighash::EcdsaSighashType::All);
            assert!(input.final_script_witness.is_none() && input.final_script_sig.is_none());
        }
        // Everything but the partial signatures is the file that came in.
        let mut stripped = out.clone();
        for i in &mut stripped.inputs {
            i.partial_sigs.clear();
        }
        assert_eq!(stripped, p);
        // Signing twice gives the same bytes (deterministic nonces).
        assert_eq!(prepared.sign(&master).unwrap(), out.serialize());
    }
}

#[test]
fn sk_56_a_signature_already_in_the_file_is_kept() {
    let master = common::master();
    let mut p = common::payment();
    // A signature of another key, as a second signer might have added.
    let other = bitcoin::PublicKey::from_str(
        "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
    )
    .unwrap();
    let sig = bitcoin::ecdsa::Signature::from_slice(&[
        0x30, 0x06, 0x02, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01,
    ])
    .unwrap();
    p.inputs[0].partial_sigs.insert(other, sig);
    let out = parsed(
        &prepare(&p.serialize(), &master)
            .unwrap()
            .sign(&master)
            .unwrap(),
    );
    assert_eq!(out.inputs[0].partial_sigs.len(), 2);
    assert!(out.inputs[0].partial_sigs.contains_key(&other));
    assert_eq!(out.inputs[1].partial_sigs.len(), 1);
}

/// Reads every frame the app shows back through the QR decoder, as the owner
/// phone would, until the file is whole.
fn read_back(a: &mut openbmt_stick::app::App<common::Zero>) -> (Form, Vec<u8>, usize) {
    let mut c = Collector::new();
    let mut seen = 0;
    for _ in 0..200 {
        let qr = a.screen().qr.expect("a code");
        let (w, h, luma) = luma_of(&qr, 5);
        let d = decode_luma(w, h, &luma).unwrap();
        seen += 1;
        if let Feed::Done { form, psbt } = c.feed(&String::from_utf8(d.bytes).unwrap()) {
            return (form, psbt, seen);
        }
        assert!(a.tick(300), "the code does not move");
    }
    panic!("never whole");
}

#[test]
fn sk_56_a_whole_run_signs_and_shows_the_file_in_the_form_it_came_in() {
    let master = common::master();
    for (p, form) in [
        (common::payment(), Form::Ur),
        (common::payment(), Form::Bbqr),
        (common::big_payment(6), Form::Ur),
        (common::big_payment(6), Form::Bbqr),
    ] {
        let mut a = scanned(&p.serialize(), form);
        type_words(&mut a, &WORDS);
        assert_eq!(a.screen().id, "sign-review");
        assert_eq!(a.key(Key::Enter), Outcome::Continue);
        let s = a.screen();
        assert_eq!(s.id, "sign-out");
        // The words are gone once the file is signed.
        let (got_form, got, _) = read_back(&mut a);
        assert_eq!(got_form, form);
        let want = prepare(&p.serialize(), &master)
            .unwrap()
            .sign(&master)
            .unwrap();
        assert_eq!(got, want);
        let back = parsed(&got);
        take_wallet_signature(&common::wallet(), &p, &back).unwrap();
        // Turn off clears the payment, the words and the frames (SK-57).
        assert!(a.holds_anything());
        assert_eq!(a.key(Key::Escape), Outcome::TurnOff);
        assert!(!a.holds_anything());
        assert_eq!(a.screen().id, "start");
    }
}

#[test]
fn sk_56_a_multi_part_code_moves_part_by_part() {
    let mut a = scanned(&common::big_payment(6).serialize(), Form::Ur);
    type_words(&mut a, &WORDS);
    a.key(Key::Enter);
    let first = a.screen();
    assert!(!a.tick(100));
    assert!(a.tick(200));
    let second = a.screen();
    assert_ne!(first.lines, second.lines);
}

#[test]
fn sk_57_turn_off_clears_the_words_the_payment_and_the_frames_at_every_stage() {
    // While the words are typed.
    let mut a = scanned(&common::payment().serialize(), Form::Ur);
    assert!(a.holds_anything());
    press(&mut a, "prim");
    a.key(Key::Escape);
    assert!(!a.holds_anything());
    // At the review, Esc refuses and clears.
    let mut b = scanned(&common::payment().serialize(), Form::Ur);
    type_words(&mut b, &WORDS);
    assert!(b.holds_anything());
    b.key(Key::Escape);
    assert!(!b.holds_anything());
    // After a refusal.
    let (_, bytes) = common::refused_payments().remove(4);
    let mut c = scanned(&bytes, Form::Ur);
    type_words(&mut c, &WORDS);
    assert_eq!(c.screen().id, "sign-refused");
    c.key(Key::Escape);
    assert!(!c.holds_anything());
}

#[test]
fn sk_58_a_signature_does_not_cover_a_changed_transaction() {
    // The signature of one transaction does not verify for another: a
    // check that the file the owner phone gets back is the one it sent.
    let master = common::master();
    let p = common::payment();
    let signed = parsed(
        &prepare(&p.serialize(), &master)
            .unwrap()
            .sign(&master)
            .unwrap(),
    );
    let mut other = signed.clone();
    other.unsigned_tx.output[0].value = bitcoin::Amount::from_sat(499_999);
    assert!(take_wallet_signature(&common::wallet(), &p, &other).is_err());
}

#[test]
fn sk_15_the_new_screens_show_every_new_word() {
    let seen = common::walk_new_screens();
    for word in [
        words::SK_50_MENU_SIGN,
        words::SK_32_MENU_FLOAT,
        words::SK_32_MENU_LIGHTNING,
        words::SK_32_FLOAT_INDEX,
        words::SK_32_FLOAT_INDEX_KEYS,
        words::SK_33_FLOAT_WHAT,
        words::SK_33_FLOAT_SPENDS,
        words::SK_33_FLOAT_SCAN,
        words::SK_33_FLOAT_NO_PHOTO,
        words::SK_33_LIGHTNING_WHAT,
        words::SK_33_LIGHTNING_SPENDS,
        words::SK_33_LIGHTNING_SCAN,
        words::SK_33_LIGHTNING_NO_PHOTO,
        words::SK_52_SCAN_TITLE,
        words::SK_52_LOOKING,
        words::SK_52_PARTS,
        words::SK_52_BACK,
        words::SK_52_NO_CAMERA,
        words::SK_52_BAD_FILE,
        words::SK_53_WORDS,
        words::SK_54_REFUSED,
        words::SK_54_INPUT,
        words::SK_54_NOT_MAINNET,
        words::SK_54_NO_PREVOUT,
        words::SK_54_PREVOUT_MISMATCH,
        words::SK_54_NO_PATH,
        words::SK_54_OTHER_WALLET,
        words::SK_54_PATH_NOT_WALLET,
        words::SK_54_KEY_MISMATCH,
        words::SK_54_NOT_SIGHASH_ALL,
        words::SK_54_OUTPUTS_EXCEED,
        words::SK_55_TITLE,
        words::SK_55_OWN,
        words::SK_55_SENT,
        words::SK_55_FEE,
        words::SK_55_FEE_RATE,
        words::SK_55_UNIT_BTC,
        words::SK_55_UNIT_RATE,
        words::SK_55_PAGE,
        words::SK_55_SIGN,
        words::SK_55_REFUSE,
        words::SK_56_SHOW,
        words::SK_56_PART,
    ] {
        assert!(seen.contains(word), "no new screen shows {word}");
    }
}

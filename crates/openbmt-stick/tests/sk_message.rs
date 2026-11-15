//! Tests of signing a settings change (SK-60 to SK-65). They run on any
//! computer; the whole boot is `stick/test-qemu.sh`.

mod common;

use common::{
    app, message_scanned, press, settings_message, signmessage, type_phrase, words24, Zero, WORDS,
};
use openbmt_core::approval::verify_wallet_message;
use openbmt_qr::{decode_luma, QrCode};
use openbmt_stick::app::App;
use openbmt_stick::keys::Key;
use openbmt_stick::message::{self, MessageError, PathKind};
use openbmt_stick::words;

/// The BIP39 test vector's words: `abandon` 11 times and `about`; its seed
/// with the passphrase TREZOR starts c55257c3.
fn vector() -> [&'static str; 12] {
    let mut w = ["abandon"; 12];
    w[11] = "about";
    w
}

const PATHS: [&str; 2] = ["m/84h/0h/0h/0/0", "m/48h/0h/0h/2h/0/0"];

/// Addresses of the vector with the passphrase TREZOR, worked out apart from
/// this crate with a short Python script that has its own BIP39 (PBKDF2),
/// BIP32, secp256k1, RIPEMD-160 and bech32 and checked against the known
/// seed prefix c55257c3 of the vector (and, without a passphrase, the known
/// first BIP84 address bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu).
const TREZOR: [(&str, &str); 4] = [
    (
        "m/84h/0h/0h/0/0",
        "bc1qv5rmq0kt9yz3pm36wvzct7p3x6mtgehjul0feu",
    ),
    (
        "m/84h/0h/7h/0/0",
        "bc1q7cyngl9fw0qp2srjyy3ermu2dkampmetcdag9x",
    ),
    (
        "m/48h/0h/0h/2h/0/0",
        "bc1q98k9lu87e3ye98xjnyhp3zfq7nce24awdycln7",
    ),
    (
        "m/48h/0h/7h/2h/0/0",
        "bc1q2w8uc9ndmxnvflpz7frgqejptc44c2cvpzycfm",
    ),
];

fn good(path: &str) -> String {
    signmessage(path, &settings_message(7, Some(2), 0xab))
}

#[test]
fn sk_60_a_settings_change_is_taken_with_a_node_or_with_none() {
    let m = message::parse(&good("m/84h/0h/0h/0/0")).unwrap();
    assert_eq!(m.kind, PathKind::Single);
    assert_eq!((m.account, m.version), (0, 7));
    assert_eq!(m.node, Some([2u8; 33]));
    assert_eq!(m.hash, [0xab; 32]);
    assert_eq!(m.text, settings_message(7, Some(2), 0xab));
    let none = signmessage("m/48h/0h/12h/2h/0/0", &settings_message(1, None, 0));
    let m = message::parse(&none).unwrap();
    assert_eq!(m.kind, PathKind::Cosigner);
    assert_eq!((m.account, m.node), (12, None));
    // The largest account and version.
    let top = signmessage(
        "m/84h/0h/2147483647h/0/0",
        &settings_message(u32::MAX, None, 0xff),
    );
    assert_eq!(message::parse(&top).unwrap().account, 2_147_483_647);
}

#[test]
fn sk_61_every_other_message_is_refused() {
    let ok = settings_message(7, Some(2), 0xab);
    let path = "m/84h/0h/0h/0/0";
    let bad_messages = [
        "hello".to_string(),
        String::new(),
        "Pay 1 BTC to bc1qxyz".to_string(),
        ok.replace("OpenBMT", "openbmt"),
        // Extra spaces.
        ok.replace("change, shop", "change,  shop"),
        ok.replace(", Lightning", " , Lightning"),
        format!("{ok} "),
        format!(" {ok}"),
        // A colon added.
        ok.replace("change,", "change:"),
        format!("{ok}:"),
        // Trailing junk.
        format!("{ok} extra"),
        format!("{ok}\n"),
        format!("{ok}00"),
        // The numbers in other forms.
        ok.replace("version 7", "version 07"),
        ok.replace("version 7", "version +7"),
        ok.replace("version 7", "version 4294967296"),
        ok.replace("version 7", "version -7"),
        ok.replace("version 7", "version "),
        ok.replace("Lightning node 0202", "Lightning node 0x02"),
        ok.replace("Lightning node", "Lightning nodes"),
        ok.to_uppercase(),
        ok.replace("ab", "AB"),
        // A hash that is short or long.
        ok[..ok.len() - 2].to_string(),
        // A node of 32 or 34 bytes, or "None".
        ok.replace(&"02".repeat(33), &"02".repeat(32)),
        ok.replace(&"02".repeat(33), &"02".repeat(34)),
        ok.replace(&"02".repeat(33), "None"),
        // Another text before the message.
        format!("note {ok}"),
    ];
    for m in &bad_messages {
        assert_eq!(
            message::parse(&signmessage(path, m)).err(),
            Some(MessageError::BadMessage),
            "{m:?}"
        );
    }
    let bad_paths = [
        "m/84h/0h/0h/0/1",
        "m/84h/0h/0h/1/0",
        "m/84h/1h/0h/0/0",
        "m/86h/0h/0h/0/0",
        "m/49h/0h/0h/0/0",
        "m/48h/0h/0h/1h/0/0",
        "m/48h/0h/0h/0/0",
        "m/48h/0h/0h/2/0/0",
        "m/84h/0h/0h/0/0/0",
        "m/84h/0h/0h/0",
        "m/84h/0h/0h",
        "m/84h/0h/0h/0h/0",
        "m/84h/0h/0h/0/0h",
        "m/84h/0h/0h/0h/0h",
        "m/84'/0'/0'/0/0",
        "m/84h/0h/0/0/0",
        "m/84h/0h/h/0/0",
        "m/84h/0h/-1h/0/0",
        "m/84h/0h/+1h/0/0",
        "m/84h/0h/01h/0/0",
        "m/84h/0h/2147483648h/0/0",
        "m/84h/0h/4294967296h/0/0",
        "m/84h/0h/99999999999h/0/0",
        "m/84h/0h/0h/0/0/",
        "/84h/0h/0h/0/0",
        "M/84h/0h/0h/0/0",
        "84h/0h/0h/0/0",
        "m/84H/0H/0H/0/0",
        "m//84h/0h/0h/0/0",
        "",
    ];
    for p in bad_paths {
        assert_eq!(
            message::parse(&signmessage(p, &ok)).err(),
            Some(MessageError::BadPath),
            "{p:?}"
        );
    }
    // The wrapper of the text.
    assert_eq!(message::parse("").err(), Some(MessageError::NotAMessage));
    assert_eq!(message::parse(&ok).err(), Some(MessageError::NotAMessage));
    assert_eq!(
        message::parse(&format!("signmessage  {path} ascii:{ok}")).err(),
        Some(MessageError::Malformed)
    );
    assert_eq!(
        message::parse(&format!("signmessage {path}  ascii:{ok}")).err(),
        Some(MessageError::Malformed)
    );
    assert_eq!(
        message::parse(&format!("signmessage {path} ascii: {ok}")).err(),
        Some(MessageError::BadMessage)
    );
    assert_eq!(
        message::parse(&format!("signmessage {path} {ok}")).err(),
        Some(MessageError::Malformed)
    );
    assert_eq!(
        message::parse(&format!("signmessage {path}")).err(),
        Some(MessageError::Malformed)
    );
    assert_eq!(
        message::parse(&format!("Signmessage {path} ascii:{ok}")).err(),
        Some(MessageError::NotAMessage)
    );
    assert_eq!(
        message::parse(&format!("signmessage {path} ascii:{ok}\n")).err(),
        Some(MessageError::BadMessage)
    );
}

#[test]
fn sk_65_the_address_for_the_test_vector_is_the_one_worked_out_apart() {
    for (path, want) in TREZOR {
        let m = message::parse(&good(path)).unwrap();
        let a = message::address_of(&vector(), "TREZOR", &m).unwrap();
        assert_eq!(a.to_string(), want, "{path}");
    }
    // A different passphrase is a different key.
    let m = message::parse(&good("m/84h/0h/0h/0/0")).unwrap();
    let plain = message::address_of(&vector(), "", &m).unwrap();
    assert_eq!(
        plain.to_string(),
        "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu"
    );
    let other = message::address_of(&vector(), "trezor", &m).unwrap();
    assert_ne!(other.to_string(), TREZOR[0].1);
    // The same words and path as the core's own wallet: the first receive
    // address of the account, as the stick's result screen shows it.
    let public = openbmt_stick::outputs::public_of(&vector()).unwrap();
    assert_eq!(public.address, plain.to_string());
}

#[test]
fn sk_65_the_signature_verifies_with_the_core_at_both_paths() {
    for (path, want) in TREZOR {
        let m = message::parse(&good(path)).unwrap();
        let sig = message::sign(&vector(), "TREZOR", &m).unwrap();
        let addr = message::address_of(&vector(), "TREZOR", &m).unwrap();
        assert_eq!(addr.to_string(), want);
        assert_eq!(
            verify_wallet_message(&addr, &m.text, &sig),
            Ok(()),
            "{path}"
        );
        // 65 bytes, header 39 to 42.
        assert_eq!(sig.len(), 88);
        let raw = base64_decode(&sig);
        assert_eq!(raw.len(), 65);
        assert!((39..=42).contains(&raw[0]), "{}", raw[0]);
        // Not valid for another message or another key.
        assert!(verify_wallet_message(&addr, &settings_message(8, Some(2), 0xab), &sig).is_err());
        let other = message::address_of(&vector(), "", &m).unwrap();
        assert!(verify_wallet_message(&other, &m.text, &sig).is_err());
    }
}

#[test]
fn sk_65_a_24_word_key_signs_too() {
    let m = message::parse(&good("m/84h/0h/3h/0/0")).unwrap();
    let w = words24();
    let sig = message::sign(&w, "", &m).unwrap();
    let addr = message::address_of(&w, "", &m).unwrap();
    assert_eq!(verify_wallet_message(&addr, &m.text, &sig), Ok(()));
    // Words that are not 12 or 24 are not a key.
    assert!(message::sign(&w[..15], "", &m).is_err());
}

fn base64_decode(text: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => 0,
    };
    let mut out = Vec::new();
    for q in text.as_bytes().chunks(4) {
        let pad = q.iter().filter(|c| **c == b'=').count();
        let n = q.iter().fold(0u32, |a, c| (a << 6) | u32::from(val(*c)));
        let b = n.to_be_bytes();
        out.extend_from_slice(&b[1..4 - pad]);
    }
    out
}

#[test]
fn sk_65_base64_is_the_standard_text() {
    for len in 0..70usize {
        let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
        let t = message::base64(&bytes);
        assert_eq!(t.len(), len.div_ceil(3) * 4);
        assert_eq!(base64_decode(&t), bytes, "{len}");
    }
    assert_eq!(message::base64(b"Man"), "TWFu");
    assert_eq!(message::base64(b"Ma"), "TWE=");
    assert_eq!(message::base64(b"M"), "TQ==");
}

#[test]
fn sk_63_the_hash_is_shown_in_groups_of_four() {
    let g = message::grouped(&[0xab; 32]);
    assert_eq!(g.split(' ').count(), 16);
    assert!(g.split(' ').all(|p| p == "abab"));
}

fn text_of(a: &App<Zero>) -> String {
    a.screen().lines.join("\n")
}

fn qr_text(qr: &QrCode) -> String {
    let scale = 6;
    let n = qr.size();
    let side = (n + 8) * scale;
    let mut luma = vec![255u8; side * side];
    for y in 0..n {
        for x in 0..n {
            if qr.get(x, y) {
                for dy in 0..scale {
                    for dx in 0..scale {
                        luma[((y + 4) * scale + dy) * side + (x + 4) * scale + dx] = 0;
                    }
                }
            }
        }
    }
    String::from_utf8(decode_luma(side, side, &luma).unwrap().bytes).unwrap()
}

#[test]
fn sk_62_to_64_a_whole_run_signs_and_then_holds_nothing() {
    let code = good(PATHS[0]);
    let mut a = message_scanned(&code);
    assert_eq!(a.screen().id, "message-words");
    assert!(a.holds_anything());
    type_phrase(&mut a, &vector());
    assert_eq!(a.screen().id, "message-passphrase");
    // The passphrase is not shown as typed.
    // The passphrase is typed as key events, Shift held for capitals.
    typed(&mut a, "TREZOR");
    assert!(!text_of(&a).contains("TREZOR"));
    assert!(text_of(&a).contains("> ******"));
    a.key(Key::Enter);
    let s = a.screen();
    assert_eq!(s.id, "message-review");
    let t = s.lines.join("\n");
    assert!(t.contains(&format!("{} 7", words::SK_63_VERSION)), "{t}");
    assert!(t.contains(&"02".repeat(33)), "{t}");
    assert!(t.contains(&message::grouped(&[0xab; 32])), "{t}");
    assert!(t.contains(TREZOR[0].1), "{t}");
    assert!(s.qr.is_none());
    // Enter signs.
    a.key(Key::Enter);
    let s = a.screen();
    assert_eq!(s.id, "message-out");
    let sig = s
        .lines
        .iter()
        .find_map(|l| l.strip_prefix(&format!("{} ", words::SK_64_TEXT)))
        .unwrap()
        .to_string();
    assert_eq!(qr_text(s.qr.as_ref().unwrap()), sig);
    let m = message::parse(&code).unwrap();
    let addr = message::address_of(&vector(), "TREZOR", &m).unwrap();
    assert_eq!(verify_wallet_message(&addr, &m.text, &sig), Ok(()));
    // Signing dropped the words, the passphrase and the message; "Turn off"
    // clears the signature too.
    assert!(a.holds_anything());
    assert_eq!(a.key(Key::Escape), openbmt_stick::app::Outcome::TurnOff);
    assert!(!a.holds_anything());
}

#[test]
fn sk_63_escape_refuses_and_nothing_is_signed() {
    let mut a = message_scanned(&good(PATHS[1]));
    type_phrase(&mut a, &vector());
    a.key(Key::Enter);
    assert_eq!(a.screen().id, "message-review");
    assert!(text_of(&a).contains("bc1q"));
    a.key(Key::Escape);
    assert_eq!(a.screen().id, "start");
    assert!(!a.holds_anything());
    // From the words and the passphrase screens too.
    let mut b = message_scanned(&good(PATHS[0]));
    type_phrase(&mut b, &vector());
    b.key(Key::Escape);
    assert_eq!(b.screen().id, "start");
    assert!(!b.holds_anything());
    let mut c = message_scanned(&good(PATHS[0]));
    press(&mut c, "aband");
    c.key(Key::Escape);
    assert!(!c.holds_anything());
}

#[test]
fn sk_62_the_words_are_12_or_24_with_the_checksum() {
    let code = good(PATHS[0]);
    // Enter on an empty line before the 12th word goes nowhere.
    let mut a = message_scanned(&code);
    type_phrase(&mut a, &WORDS[..5]);
    assert_eq!(a.screen().id, "message-words");
    // 12 words that fail the checksum: the error shows, the words stay.
    let mut b = message_scanned(&code);
    type_phrase(&mut b, &["abandon"; 12]);
    assert_eq!(b.screen().id, "message-words");
    assert!(text_of(&b).contains(words::SK_62_BAD_CHECKSUM));
    // ... and the 12th word is not the last that can be typed: more words
    // make 24, which pass.
    let mut c = message_scanned(&code);
    type_phrase(&mut c, &words24()[..12]);
    assert!(text_of(&c).contains(words::SK_62_BAD_CHECKSUM));
    type_phrase(&mut c, &words24()[12..]);
    assert_eq!(c.screen().id, "message-passphrase");
    let mut d = message_scanned(&code);
    type_phrase(&mut d, &words24());
    assert_eq!(d.screen().id, "message-passphrase");
    // A word of the first four letters.
    let mut e = message_scanned(&code);
    for w in vector() {
        press(&mut e, &w[..4]);
        e.key(Key::Enter);
    }
    e.key(Key::Enter);
    assert_eq!(e.screen().id, "message-passphrase");
    // Backspace takes the last word back.
    let mut f = message_scanned(&code);
    type_words(&mut f, &["abandon", "abandon"]);
    f.key(Key::Backspace);
    assert!(text_of(&f).contains("> abandon"));
    // No 25th word.
    let mut g = message_scanned(&code);
    let mut w25 = words24();
    w25.push("abandon");
    type_words(&mut g, &w25);
    assert!(text_of(&g).contains(&format!("{} 25", words::SK_28_TYPE)));
}

fn type_words(a: &mut App<Zero>, words: &[&str]) {
    common::type_words(a, words);
}

#[test]
fn sk_61_a_refused_code_shows_the_refusal_and_asks_for_no_words() {
    let ok = settings_message(7, Some(2), 0xab);
    for code in [
        signmessage("m/84h/0h/0h/0/0", "hello"),
        signmessage("m/84h/0h/1h/0/0", &format!("{ok} ")),
        signmessage("m/44h/0h/0h/0/0", &ok),
        signmessage("m/84h/0h/2147483648h/0/0", &ok),
        "signmessage".to_string(),
        "signmessage m/84h/0h/0h/0/0".to_string(),
    ] {
        let mut a = message_scanned(&code);
        let s = a.screen();
        assert_eq!(s.id, "message-refused", "{code:?}");
        assert!(s.lines.contains(&words::SK_61_REFUSED.to_string()));
        assert!(!a.holds_anything());
        // Keys do nothing but turn off.
        a.key(Key::Enter);
        assert_eq!(a.screen().id, "message-refused");
        assert_eq!(a.key(Key::Escape), openbmt_stick::app::Outcome::TurnOff);
    }
}

#[test]
fn sk_60_a_payment_is_still_read_in_the_same_mode() {
    let mut a = app();
    a.key(Key::Char('4'));
    a.camera_found();
    // A plain text that is no part of a file changes nothing.
    assert!(!a.frame_text("https://example.invalid/x"));
    assert_eq!(a.screen().id, "sign-scan");
    // A message read after a part of a payment replaces it.
    assert!(a.frame_text(&good(PATHS[0])));
    assert_eq!(a.screen().id, "message-words");
}

/// Types `text` as a keyboard does: the key events of each character, with
/// Shift pressed and released around the characters that need it.
fn typed(a: &mut App<Zero>, text: &str) {
    use openbmt_stick::keys::{key_from_code_shift, Decoder, EVENT_SIZE, SHIFT_CODES};
    let ev = |code: u16, value: i32| {
        let mut raw = [0u8; EVENT_SIZE];
        raw[16..18].copy_from_slice(&1u16.to_ne_bytes());
        raw[18..20].copy_from_slice(&code.to_ne_bytes());
        raw[20..24].copy_from_slice(&value.to_ne_bytes());
        raw
    };
    let mut d = Decoder::new();
    for c in text.chars() {
        let (code, shift) = (0..100u16)
            .flat_map(|k| [(k, false), (k, true)])
            .find(|(k, s)| key_from_code_shift(*k, *s) == Some(Key::Char(c)))
            .unwrap();
        let mut events = Vec::new();
        if shift {
            events.push(ev(SHIFT_CODES[1], 1));
        }
        events.push(ev(code, 1));
        events.push(ev(code, 0));
        if shift {
            events.push(ev(SHIFT_CODES[1], 0));
        }
        for e in &events {
            if let Some(k) = d.event(e) {
                a.key(k);
            }
        }
    }
}

#[test]
fn sk_62_the_passphrase_screen_takes_capitals_spaces_and_signs() {
    let m = message::parse(&good(PATHS[0])).unwrap();
    for pass in [
        "TREZOR",
        "a b!",
        "TREZOR 1!",
        "~`!@#$%^&*()-_=+[{]}\\|;:'\",<.>/?",
    ] {
        let mut a = message_scanned(&good(PATHS[0]));
        type_phrase(&mut a, &vector());
        typed(&mut a, pass);
        let t = text_of(&a);
        assert!(t.contains(&format!("> {}", "*".repeat(pass.len()))), "{t}");
        a.key(Key::Enter);
        let want = message::address_of(&vector(), pass, &m)
            .unwrap()
            .to_string();
        assert!(text_of(&a).contains(&want), "{pass:?}");
    }
    // Backspace erases one character.
    let mut a = message_scanned(&good(PATHS[0]));
    type_phrase(&mut a, &vector());
    typed(&mut a, "ab c");
    a.key(Key::Backspace);
    assert!(text_of(&a).contains("> ***\n"));
    // Word entry keeps lower-case letters: a capital or a space is ignored.
    let mut b = message_scanned(&good(PATHS[0]));
    b.key(Key::Char('A'));
    b.key(Key::Char(' '));
    b.key(Key::Char('!'));
    assert!(text_of(&b).contains("> \n"));
}

/// How many rows a screen needs and how many the display has, as `render`
/// lays it out (`draw.rs`): the same margin, scale, columns and wrapping.

#[test]
fn sk_14_the_settings_change_screens_fit_the_display() {
    let code = signmessage(
        "m/84h/0h/0h/0/0",
        &settings_message(4_000_000_000, Some(2), 0xab),
    );
    let mut screens = Vec::new();
    let mut a = message_scanned(&code);
    screens.push(a.screen());
    type_phrase(&mut a, &vector());
    screens.push(a.screen());
    typed(&mut a, &"x".repeat(100));
    screens.push(a.screen());
    a.key(Key::Enter);
    screens.push(a.screen());
    a.key(Key::Enter);
    screens.push(a.screen());
    screens.push(message_scanned("signmessage x").screen());
    let mut c = message_scanned(&code);
    type_phrase(&mut c, &["abandon"; 12]);
    screens.push(c.screen());
    // Every row is drawn down to the last line (Enter and Esc) on every
    // display, the text drawn smaller where it would not fit; from 1024 x 768
    // up it stays at the full size.
    for (w, h) in [
        (1920, 1080),
        (1366, 768),
        (1280, 800),
        (1024, 768),
        (800, 600),
        (640, 480),
    ] {
        for s in &screens {
            let f = openbmt_stick::draw::fit(s, w, h);
            assert!(f.rows_needed <= f.rows_free, "{w}x{h} {} {f:?}", s.id);
            if w >= 1024 {
                let full = if s.qr.is_some() {
                    (openbmt_stick::draw::text_scale(w) / 2).max(1)
                } else {
                    openbmt_stick::draw::text_scale(w)
                };
                assert_eq!(f.scale, full, "{w}x{h} {}", s.id);
            }
        }
    }
}

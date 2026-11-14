//! Every bound and every refusal, each with its rule code in the text.

use super::{make_message, part_text};
use crate::bytewords::{decode_minimal, encode_minimal};
use crate::cbor::{decode_bytes, encode_bytes};
use crate::fountain::{choose_fragments, encode_part};
use crate::hash::crc32;
use crate::{
    psbt_to_ur_parts, ur_to_psbt, Decoder, Encoder, UrError, MAX_MESSAGE_LEN, MAX_TEXT_LEN,
};

/// The rule code a refusal starts with, and that its text does too.
fn code<T: std::fmt::Debug>(r: Result<T, UrError>) -> String {
    let e = r.expect_err("a refusal");
    let text = e.to_string();
    assert!(text.starts_with(e.rule()), "{text}");
    e.rule().to_string()
}

/// A decoder that has taken part 1 of a two-part message of 15 bytes.
fn started() -> (Decoder, Vec<u8>, u32) {
    let message = make_message(15, "start");
    let checksum = crc32(&message);
    let mut d = Decoder::new();
    let mut frag = message.get(..10).expect("ten").to_vec();
    d.receive(&part_text("bytes", 1, 2, 15, checksum, &frag))
        .expect("a good first part");
    frag.clear();
    (d, message, checksum)
}

#[test]
fn ur_01_type_is_one_to_sixty_four_of_lower_case_digits_and_dash() {
    let cbor = encode_bytes(&[1, 2, 3]);
    for ok in ["bytes", "crypto-psbt", "a", "x-1-2", &"a".repeat(64)] {
        assert!(Encoder::new(ok, &cbor, 100, 10).is_ok(), "{ok}");
    }
    for bad in ["", "Bytes", "by_tes", "by tes", "é", "a/b", &"a".repeat(65)] {
        assert_eq!(code(Encoder::new(bad, &cbor, 100, 10)), "UR-01", "{bad:?}");
    }
    // The decoder holds the same line.
    let body = encode_minimal(&[1, 2, 3]);
    assert_eq!(
        code(Decoder::new().receive(&format!("ur:/{body}"))),
        "UR-01"
    );
    assert_eq!(
        code(Decoder::new().receive(&format!("ur:by_tes/{body}"))),
        "UR-01"
    );
    assert_eq!(
        code(Decoder::new().receive(&format!("ur:{}/{body}", "a".repeat(65)))),
        "UR-01"
    );
}

#[test]
fn ur_02_message_is_one_byte_to_one_mebibyte() {
    assert_eq!(code(Encoder::new("bytes", &[], 100, 10)), "UR-02");
    let big = vec![7u8; MAX_MESSAGE_LEN + 1];
    assert_eq!(code(Encoder::new("bytes", &big, 2000, 10)), "UR-02");
    assert!(Encoder::new("bytes", &big[1..], 2000, 10).is_ok());
    // On the way in: a part that says 0 or too many bytes.
    let data = [0u8; 10];
    let zero = part_text("bytes", 1, 1, 0, 0, &data);
    assert_eq!(code(Decoder::new().receive(&zero)), "UR-02");
    let huge = part_text("bytes", 1, 1, MAX_MESSAGE_LEN + 1, 0, &data);
    assert_eq!(code(Decoder::new().receive(&huge)), "UR-02");
    // A whole UR of 1 MiB is read; one byte more is refused.
    let whole = encode_minimal(&vec![9u8; MAX_MESSAGE_LEN]);
    let mut d = Decoder::new();
    assert!(
        d.receive(&format!("ur:bytes/{whole}"))
            .expect("ok")
            .complete
    );
    let over = encode_minimal(&vec![9u8; MAX_MESSAGE_LEN + 1]);
    assert_eq!(
        code(Decoder::new().receive(&format!("ur:bytes/{over}"))),
        "UR-02"
    );
}

#[test]
fn ur_03_seq_len_is_one_to_ten_thousand() {
    let data = [0u8; 10];
    for n in [0usize, 10_001, 70_000] {
        let t = part_text("bytes", 1, n, 100, 0, &data);
        assert_eq!(code(Decoder::new().receive(&t)), "UR-03", "{n}");
    }
    // The encoder: 10,001 fragments of 100 bytes.
    let msg = vec![1u8; 100 * 10_001];
    assert_eq!(code(Encoder::new("bytes", &msg, 100, 10)), "UR-03");
    let msg = vec![1u8; 100 * 10_000];
    let e = Encoder::new("bytes", &msg, 100, 10).expect("10,000 fit");
    assert_eq!(e.seq_len(), 10_000);
}

#[test]
fn ur_04_fragment_length_is_ten_to_two_thousand() {
    let cbor = encode_bytes(&[1; 500]);
    for max in [0usize, 9, 2001, 5000] {
        assert_eq!(
            code(Encoder::new("bytes", &cbor, max, 10)),
            "UR-04",
            "{max}"
        );
    }
    for (max, min) in [(100usize, 9usize), (100, 101), (100, 0)] {
        assert_eq!(code(Encoder::new("bytes", &cbor, max, min)), "UR-04");
    }
    assert!(Encoder::new("bytes", &encode_bytes(&[1; 98]), 10, 10).is_ok());
    assert!(Encoder::new("bytes", &cbor, 2000, 10).is_ok());
    // A range that no fragment length fits.
    let cbor = encode_bytes(&[1; 17]);
    assert_eq!(code(Encoder::new("bytes", &cbor, 10, 10)), "UR-04");
    // The decoder: fragments of 9 and 2001 bytes.
    for len in [9usize, 2001] {
        let t = part_text("bytes", 1, 2, len + 1, 0, &vec![0u8; len]);
        assert_eq!(code(Decoder::new().receive(&t)), "UR-04", "{len}");
    }
}

#[test]
fn ur_05_bytewords_are_minimal_pairs_of_letters() {
    // Odd length, too short, a pair that is no word's, a non-letter.
    for bad in [
        "aeadaolazmjendeot",
        "aeadaola",
        "",
        "aeadaolazmjendeozz",
        "ae ad",
    ] {
        assert_eq!(code(decode_minimal(bad)), "UR-05", "{bad:?}");
    }
    // Not ASCII: no slicing of the text can fail.
    for bad in [
        "éééééééééé",
        "aeadaolazmjendeo\u{1F600}",
        "日本語日本語日本語",
    ] {
        assert_eq!(code(decode_minimal(bad)), "UR-05", "{bad:?}");
    }
    assert_eq!(code(Decoder::new().receive("ur:bytes/zzzzzzzzzz")), "UR-05");
}

#[test]
fn ur_06_bytewords_checksum_must_match() {
    // The last pair changed to another valid pair.
    assert_eq!(code(decode_minimal("aeadaolazmjendeoae")), "UR-06");
    // A changed byte in the body.
    assert_eq!(
        code(decode_minimal(
            "aeadaolazmjendeoti".replace("ao", "ae").as_str()
        )),
        "UR-06"
    );
    let good = encode_minimal(&[5, 6, 7, 8, 9, 10]);
    assert!(decode_minimal(&good).is_ok());
    // The same through a UR.
    let mut bad = good.clone();
    bad.replace_range(..2, "zm");
    assert_eq!(
        code(Decoder::new().receive(&format!("ur:bytes/{bad}"))),
        "UR-06"
    );
}

#[test]
fn ur_07_text_is_ur_type_body_or_ur_type_n_m_body() {
    let body = encode_minimal(&[1, 2, 3]);
    for bad in [
        String::new(),
        "ur:".to_string(),
        "ur:bytes".to_string(),
        format!("bytes/{body}"),
        format!("ur:bytes/1-2/{body}/more"),
        format!("ur:bytes/1/{body}"),
        format!("ur:bytes/1-2-3/{body}"),
        format!("ur:bytes/-2/{body}"),
        format!("ur:bytes/1-/{body}"),
        format!("ur:bytes/+1-2/{body}"),
        format!("ur:bytes/01-2/{body}"),
        format!("ur:bytes/1-2e0/{body}"),
        format!("ur:bytes/99999999999-2/{body}"),
        format!("ur:bytes/a-b/{body}"),
        format!("xx:bytes/{body}"),
    ] {
        assert_eq!(code(Decoder::new().receive(&bad)), "UR-07", "{bad:?}");
    }
}

#[test]
fn ur_08_multipart_body_is_the_strict_five_item_array() {
    let data = vec![3u8; 10];
    // Helper: the text around a body.
    let wrap = |body: &[u8]| format!("ur:bytes/1-2/{}", encode_minimal(body));
    let good = encode_part(1, 2, 15, 7, &data);
    assert!(Decoder::new().receive(&wrap(&good)).is_ok());
    // Not an array of five: four, six, a map, a byte string, indefinite.
    let mut four = good.clone();
    if let Some(b) = four.first_mut() {
        *b = 0x84;
    }
    let mut six = good.clone();
    if let Some(b) = six.first_mut() {
        *b = 0x86;
    }
    let mut indefinite = good.clone();
    if let Some(b) = indefinite.first_mut() {
        *b = 0x9f;
    }
    // Trailing bytes after the data, and the data cut short.
    let mut trailing = good.clone();
    trailing.push(0);
    let cut = good.get(..good.len() - 1).expect("long").to_vec();
    // A number that is not shortest (seqNum 1 written in two bytes).
    let mut wide = vec![0x85, 0x18, 0x01];
    wide.extend_from_slice(good.get(2..).expect("long"));
    // The data as text instead of bytes.
    let mut text = good.clone();
    let at = text.len() - 11;
    if let Some(b) = text.get_mut(at) {
        *b = 0x6a;
    }
    // A negative number where a number goes.
    let mut negative = good.clone();
    if let Some(b) = negative.get_mut(1) {
        *b = 0x20;
    }
    for (name, bad) in [
        ("four", four),
        ("six", six),
        ("indefinite", indefinite),
        ("trailing", trailing),
        ("cut", cut),
        ("wide", wide),
        ("text", text),
        ("negative", negative),
        ("empty", Vec::new()),
        ("tag", vec![0xd8, 0x28, 0x85]),
    ] {
        // `empty` and `tag` fail in the Bytewords already when too short;
        // the rest reach the envelope.
        let r = Decoder::new().receive(&wrap(&bad));
        assert!(
            matches!(&r, Err(e) if e.rule() == "UR-08" || e.rule() == "UR-05"),
            "{name}: {r:?}"
        );
        if name != "empty" {
            assert!(r.is_err(), "{name}");
        }
    }
    let r = Decoder::new().receive(&wrap(&[0x85, 0x01]));
    assert_eq!(code(r), "UR-08");
}

#[test]
fn ur_09_part_fields_must_fit_one_message() {
    let data = vec![3u8; 10];
    // The text and the body disagree on the sequence.
    let t = format!(
        "ur:bytes/2-2/{}",
        encode_minimal(&encode_part(1, 2, 15, 7, &data))
    );
    assert_eq!(code(Decoder::new().receive(&t)), "UR-09");
    let t = format!(
        "ur:bytes/1-3/{}",
        encode_minimal(&encode_part(1, 2, 15, 7, &data))
    );
    assert_eq!(code(Decoder::new().receive(&t)), "UR-09");
    // Sequence number 0.
    let t = format!(
        "ur:bytes/0-2/{}",
        encode_minimal(&encode_part(0, 2, 15, 7, &data))
    );
    assert_eq!(code(Decoder::new().receive(&t)), "UR-09");
    // A number over 32 bits.
    let mut body = vec![0x85, 0x01, 0x02, 0x0f, 0x1b];
    body.extend_from_slice(&u64::MAX.to_be_bytes());
    body.push(0x4a);
    body.extend_from_slice(&data);
    let t = format!("ur:bytes/1-2/{}", encode_minimal(&body));
    assert_eq!(code(Decoder::new().receive(&t)), "UR-09");
    // Fragments that do not cover the message, or leave the last empty.
    let short = part_text("bytes", 1, 2, 21, 7, &data);
    assert_eq!(code(Decoder::new().receive(&short)), "UR-09");
    let spare = part_text("bytes", 1, 3, 15, 7, &data);
    assert_eq!(code(Decoder::new().receive(&spare)), "UR-09");
}

#[test]
fn ur_10_a_part_must_agree_with_the_first_accepted() {
    let (mut d, message, checksum) = started();
    let frag2 = {
        let mut f = message.get(10..).expect("tail").to_vec();
        f.resize(10, 0);
        f
    };
    let other = make_message(15, "other");
    for (name, text) in [
        ("type", part_text("psbt", 2, 2, 15, checksum, &frag2)),
        ("seqLen", part_text("bytes", 2, 3, 25, checksum, &frag2)),
        ("messageLen", part_text("bytes", 2, 2, 14, checksum, &frag2)),
        (
            "checksum",
            part_text("bytes", 2, 2, 15, crc32(&other), &frag2),
        ),
        (
            "fragment",
            part_text("bytes", 2, 2, 15, checksum, &[0u8; 11]),
        ),
    ] {
        let r = d.receive(&text);
        assert!(matches!(&r, Err(e) if e.rule() == "UR-10"), "{name}: {r:?}");
    }
    // A whole UR among multi-part ones, too.
    let whole = format!("ur:bytes/{}", encode_minimal(&[1, 2, 3]));
    assert_eq!(code(d.receive(&whole)), "UR-10");
    // After all that, the second part still finishes it.
    let done = d
        .receive(&part_text("bytes", 2, 2, 15, checksum, &frag2))
        .expect("good");
    assert!(done.complete);
    assert_eq!(d.result().map(|r| r.1), Some(message));
    // Finished, a part of another type is refused.
    assert_eq!(code(d.receive(&whole.replace("bytes", "psbt"))), "UR-10");
}

#[test]
fn ur_11_mixed_parts_held_are_at_most_twice_the_sequence_length() {
    let seq_len = 12;
    let checksum = 0x1234_5678;
    let data = [5u8; 10];
    let mut d = Decoder::new();
    // Parts of degree two to half the sequence (no more is held), so that
    // nothing is ever learned.
    let mut seen = Vec::new();
    let mut most = 0;
    for n in (seq_len as u32 + 1)..400_000 {
        let idx = choose_fragments(n, seq_len, checksum);
        if idx.len() < 2 || idx.len() > seq_len / 2 || seen.contains(&idx) {
            continue;
        }
        seen.push(idx);
        let text = part_text("bytes", n, seq_len, 111, checksum, &data);
        // A part beyond the bound is not held (or pushes a worse one out).
        d.receive(&text).expect("never an error for the bound");
        most = most.max(d.held_mixed());
        assert!(d.held_mixed() <= 2 * seq_len);
    }
    assert_eq!(most, 2 * seq_len);
    assert_eq!(d.held_mixed(), 2 * seq_len);
}

#[test]
fn ur_12_the_joined_message_must_match_its_checksum_and_padding() {
    // A wrong checksum: the stream cannot mend it, so the decoder restarts.
    let message = make_message(15, "check");
    let mut d = Decoder::new();
    let a = message.get(..10).expect("ten").to_vec();
    let mut b = message.get(10..).expect("tail").to_vec();
    b.resize(10, 0);
    d.receive(&part_text("bytes", 1, 2, 15, crc32(&message) ^ 1, &a))
        .expect("first");
    let r = d.receive(&part_text("bytes", 2, 2, 15, crc32(&message) ^ 1, &b));
    assert_eq!(code(r), "UR-12");
    assert!(!d.is_complete());
    // The decoder is clean: a good stream of the same message completes.
    d.receive(&part_text("bytes", 1, 2, 15, crc32(&message), &a))
        .expect("first");
    let done = d
        .receive(&part_text("bytes", 2, 2, 15, crc32(&message), &b))
        .expect("second");
    assert!(done.complete);
    // Padding that is not zeros, with a checksum of the message alone.
    let mut d = Decoder::new();
    let mut dirty = b.clone();
    if let Some(x) = dirty.last_mut() {
        *x = 1;
    }
    d.receive(&part_text("bytes", 1, 2, 15, crc32(&message), &a))
        .expect("first");
    let r = d.receive(&part_text("bytes", 2, 2, 15, crc32(&message), &dirty));
    assert_eq!(code(r), "UR-12");
}

#[test]
fn ur_13_cbor_byte_string_is_strict() {
    assert_eq!(decode_bytes(&[0x43, 1, 2, 3]), Ok([1, 2, 3].as_slice()));
    assert_eq!(decode_bytes(&[0x40]), Ok([].as_slice()));
    let long = vec![9u8; 300];
    assert_eq!(
        encode_bytes(&long).get(..3),
        Some([0x59, 0x01, 0x2c].as_slice())
    );
    assert_eq!(decode_bytes(&encode_bytes(&long)), Ok(long.as_slice()));
    let medium = vec![9u8; 24];
    assert_eq!(encode_bytes(&medium).get(..2), Some([0x58, 24].as_slice()));
    for bad in [
        vec![],
        vec![0x43, 1, 2],
        vec![0x43, 1, 2, 3, 4],
        vec![0x58, 0x03, 1, 2, 3],
        vec![0x59, 0x00, 0x03, 1, 2, 3],
        vec![0x5f, 0xff],
        vec![0x63, b'a', b'b', b'c'],
        vec![0x5c],
        vec![0x59, 0x01],
        vec![0x5b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1],
        vec![0xd8, 0x28, 0x43, 1, 2, 3],
    ] {
        assert_eq!(code(decode_bytes(&bad)), "UR-13", "{bad:?}");
    }
}

#[test]
fn ur_14_psbt_types_and_magic() {
    let mut psbt = b"psbt\xff".to_vec();
    psbt.extend_from_slice(&[0u8; 60]);
    let cbor = encode_bytes(&psbt);
    assert_eq!(ur_to_psbt("crypto-psbt", &cbor), Ok(psbt.clone()));
    assert_eq!(ur_to_psbt("psbt", &cbor), Ok(psbt.clone()));
    assert_eq!(code(ur_to_psbt("bytes", &cbor)), "UR-14");
    assert_eq!(code(ur_to_psbt("crypto-account", &cbor)), "UR-14");
    assert_eq!(
        code(ur_to_psbt("crypto-psbt", &encode_bytes(b"nope\xff00"))),
        "UR-14"
    );
    assert_eq!(code(ur_to_psbt("crypto-psbt", &[0x43, 1, 2])), "UR-13");
    assert_eq!(code(psbt_to_ur_parts(b"nope", 100)), "UR-14");
    assert_eq!(code(psbt_to_ur_parts(b"", 100)), "UR-14");
    let mut e = psbt_to_ur_parts(&psbt, 100).expect("good");
    assert!(e.next_part().starts_with("ur:crypto-psbt/"));
}

#[test]
fn ur_16_text_is_at_most_the_longest_ur_within_the_bounds() {
    let long = format!("ur:bytes/{}", "a".repeat(MAX_TEXT_LEN));
    assert_eq!(code(Decoder::new().receive(&long)), "UR-16");
    assert_eq!(
        code(Decoder::new().receive(&"x".repeat(MAX_TEXT_LEN + 1))),
        "UR-16"
    );
    // The longest legal UR, a whole 1 MiB message with a long type, fits.
    let whole = format!(
        "ur:{}/{}",
        "a".repeat(64),
        encode_minimal(&vec![1u8; MAX_MESSAGE_LEN])
    );
    assert!(whole.len() <= MAX_TEXT_LEN);
    assert!(Decoder::new().receive(&whole).is_ok());
}

/// Parts of the stream of `seq_len` (checksum `checksum`) with a degree in
/// `degrees`, as many as `want`, from the sequence numbers after the pure ones.
fn parts_of_degree(
    seq_len: usize,
    checksum: u32,
    degrees: std::ops::RangeInclusive<usize>,
    want: usize,
) -> Vec<u32> {
    let chooser = crate::fountain::Chooser::new(seq_len, checksum);
    (seq_len as u32 + 1..)
        .filter(|&n| degrees.contains(&chooser.choose(n).len()))
        .take(want)
        .collect()
}

#[test]
fn ur_11_hostile_high_degree_parts_hold_bounded_entries_and_work() {
    // The proportions of the largest stream (10,000 parts of 105 bytes, a
    // message of about 1 MiB), at a tenth of the size.
    let seq_len = 1000;
    let fragment_len = 105;
    let message_len = seq_len * fragment_len - 3;
    let checksum = 0x0bad_f00d;
    let data = vec![9u8; fragment_len];
    let budget = 100 * seq_len;
    // Degrees above half the sequence are never held, however many come.
    let mut d = Decoder::new();
    for n in parts_of_degree(seq_len, checksum, seq_len / 2 + 1..=seq_len, 2 * seq_len) {
        let text = part_text("bytes", n, seq_len, message_len, checksum, &data);
        d.receive(&text).expect("dropped, not refused");
    }
    assert_eq!(d.held_mixed(), 0);
    assert_eq!(d.held_entries_and_work(), (0, 0));
    // Degrees just under the cap fill the entry budget, and no more.
    for n in parts_of_degree(seq_len, checksum, seq_len / 4..=seq_len / 2, 2 * seq_len) {
        let text = part_text("bytes", n, seq_len, message_len, checksum, &data);
        d.receive(&text)
            .expect("pushed out or not held, not refused");
        assert!(d.held_entries_and_work().0 <= budget);
    }
    // The pure fragments then reduce every held part, each entry once.
    let (entries, _) = d.held_entries_and_work();
    assert!(entries > budget / 2, "{entries}");
    let mut work = 0;
    for n in 1..=seq_len as u32 {
        let text = part_text("bytes", n, seq_len, message_len, checksum, &data);
        // The last one completes a message that fails its checksum.
        let _ = d.receive(&text);
        work = work.max(d.held_entries_and_work().1);
    }
    assert!(work <= budget, "{work} looks at held entries");
}

#[test]
fn ur_11_an_honest_stream_of_the_largest_length_decodes_within_the_budget() {
    // 10,000 fragments of 104 bytes: the longest sequence UR-03 allows.
    let message = make_message(10_000 * 104 - 20, "largest");
    let payload = crate::cbor::encode_bytes(&message);
    let mut e = Encoder::new("bytes", &payload, 104, 10).expect("ok");
    assert_eq!(e.seq_len(), 10_000);
    // 30% of the parts lost, in order.
    let mut d = Decoder::new();
    let mut max_entries = 0;
    for i in 0..40_000 {
        let p = e.next_part();
        if i % 10 < 3 {
            continue;
        }
        d.receive(&p).expect("no refusal for the bound");
        max_entries = max_entries.max(d.held_entries_and_work().0);
        if d.is_complete() {
            break;
        }
    }
    assert!(d.is_complete(), "not complete");
    assert!(max_entries <= 100 * 10_000);
    assert_eq!(d.result().map(|r| r.1), Some(payload));
}

/// A scanner that starts late on an honest animated sequence sees only mixed
/// parts (sequence numbers above the sequence length). Returns the parts
/// taken, the most index entries held, and whether a part was refused for
/// the bound.
fn read_from_the_first_mix(seq_len: usize, lost_in_ten: usize) -> (usize, usize, bool) {
    let message = make_message(seq_len * 100 - 20, "late");
    let payload = crate::cbor::encode_bytes(&message);
    let mut e = Encoder::new("bytes", &payload, 100, 10).expect("ok");
    assert_eq!(e.seq_len(), seq_len);
    for _ in 0..seq_len {
        e.next_part();
    }
    let mut d = Decoder::new();
    let (mut taken, mut max_entries, mut full) = (0, 0, false);
    for i in 0..60 * seq_len {
        let p = e.next_part();
        if i % 10 < lost_in_ten {
            continue;
        }
        taken += 1;
        match d.receive(&p) {
            Ok(_) => {}
            Err(err) => {
                assert_eq!(err.rule(), "UR-11", "{err}");
                full = true;
            }
        }
        max_entries = max_entries.max(d.held_entries_and_work().0);
        if d.is_complete() {
            assert_eq!(d.result().map(|r| r.1), Some(payload));
            return (taken, max_entries, full);
        }
    }
    panic!("{seq_len} parts, {lost_in_ten}0% lost: not complete after {taken} parts");
}

fn late_start(seq_len: usize) {
    for lost in [0, 3] {
        let (_taken, max_entries, full) = read_from_the_first_mix(seq_len, lost);
        assert!(max_entries <= 100 * seq_len);
        assert!(!full, "an honest stream hit the bound");
    }
}

#[test]
fn ur_11_a_late_start_on_an_honest_stream_completes_at_50() {
    late_start(50);
}

#[test]
fn ur_11_a_late_start_on_an_honest_stream_completes_at_100() {
    late_start(100);
}

#[test]
fn ur_11_a_late_start_on_an_honest_stream_completes_at_1000() {
    late_start(1000);
}

// About 40 s with the dev profile; run with `--run-ignored all`.
#[test]
#[ignore]
fn ur_11_a_late_start_on_an_honest_stream_completes_at_10000() {
    late_start(10_000);
}

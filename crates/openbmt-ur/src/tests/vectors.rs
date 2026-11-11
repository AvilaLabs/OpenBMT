//! The published BCR-2020-005 and BCR-2020-012 vectors that are included,
//! and the pins taken from this code (marked as such) for the rest.

use super::make_message;
use crate::bytewords::{decode_minimal, encode_minimal, word};
use crate::fountain::{choose_fragments, nominal_fragment_len};
use crate::hash::crc32;
use crate::xoshiro::Xoshiro256;
use crate::{cbor, Decoder, Encoder};

#[test]
fn ur_15_xoshiro_wolf_words_match_the_reference() {
    let mut rng = Xoshiro256::from_bytes(b"Wolf");
    let got: Vec<u64> = (0..41).map(|_| rng.next_u64() % 100).collect();
    let want: [u64; 41] = [
        42, 81, 85, 8, 82, 84, 76, 73, 70, 88, 2, 74, 40, 48, 77, 54, 88, 7, 5, 88, 37, 25, 82, 13,
        69, 59, 30, 39, 11, 82, 19, 99, 45, 87, 30, 15, 32, 22, 89, 44, 92,
    ];
    assert_eq!(got, want);
}

#[test]
fn ur_15_xoshiro_wolf_ints_match_the_reference() {
    let mut rng = Xoshiro256::from_bytes(b"Wolf");
    let got: Vec<u64> = (0..19).map(|_| rng.next_int(1, 10)).collect();
    let want: [u64; 19] = [6, 5, 8, 4, 10, 5, 7, 10, 4, 9, 10, 9, 7, 7, 1, 1, 2, 9, 9];
    assert_eq!(got, want);
}

#[test]
fn ur_15_the_fragment_choices_match_the_reference() {
    // Message 1024 bytes from "Wolf", 11 fragments of 100: the fragments of
    // parts 1 to 30.
    let message = make_message(1024, "Wolf");
    let checksum = crc32(&message);
    assert_eq!(nominal_fragment_len(1024, 10, 100), 94);
    let want: [&[usize]; 30] = [
        &[0],
        &[1],
        &[2],
        &[3],
        &[4],
        &[5],
        &[6],
        &[7],
        &[8],
        &[9],
        &[10],
        &[9],
        &[2, 5, 6, 8, 9, 10],
        &[8],
        &[1, 5],
        &[1],
        &[0, 2, 4, 5, 8, 10],
        &[5],
        &[2],
        &[2],
        &[0, 1, 3, 4, 5, 7, 9, 10],
        &[0, 1, 2, 3, 5, 6, 8, 9, 10],
        &[0, 2, 4, 5, 7, 8, 9, 10],
        &[3, 5],
        &[4],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        &[0, 1, 3, 4, 5, 6, 7, 9, 10],
        &[6],
        &[5, 6],
        &[7],
    ];
    for (n, want) in (1u32..).zip(want) {
        assert_eq!(choose_fragments(n, 11, checksum), want, "part {n}");
    }
}

#[test]
fn ur_05_the_published_bytewords_example() {
    // BCR-2020-012: the bytes 00 01 02 80 ff with their CRC-32.
    let bytes = [0x00, 0x01, 0x02, 0x80, 0xff];
    let crc = crc32(&bytes).to_be_bytes();
    let words: Vec<&str> = bytes.iter().chain(&crc).map(|&b| word(b)).collect();
    assert_eq!(
        words.join(" "),
        "able acid also lava zoom jade need echo taxi"
    );
    assert_eq!(encode_minimal(&bytes), "aeadaolazmjendeoti");
    assert_eq!(decode_minimal("aeadaolazmjendeoti"), Ok(bytes.to_vec()));
    assert_eq!(
        decode_minimal("AEADAOLAZMJENDEOTI"),
        Ok(bytes.to_vec()),
        "upper case reads"
    );
}

#[test]
fn ur_05_the_word_list_is_the_registry_list() {
    assert_eq!(word(0), "able");
    assert_eq!(word(128), "lava");
    assert_eq!(word(255), "zoom");
    let words: Vec<&str> = (0..=255u8).map(word).collect();
    let mut sorted = words.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted, words, "alphabetical and without repeats");
    assert!(words.iter().all(|w| w.len() == 4));
    // The minimal style is only sound when first and last letters tell the
    // words apart.
    let mut pairs: Vec<(u8, u8)> = words
        .iter()
        .map(|w| (w.as_bytes()[0], w.as_bytes()[3]))
        .collect();
    pairs.sort_unstable();
    pairs.dedup();
    assert_eq!(pairs.len(), 256);
    for b in 0..=255u8 {
        let one = encode_minimal(&[b, 1, 2, 3, 4]);
        assert_eq!(
            decode_minimal(&one).map(|v| v.first().copied()),
            Ok(Some(b))
        );
    }
}

/// The reference multi-part example: 256 bytes from "Wolf" in a CBOR byte
/// string, parts of at most 30 bytes, type `bytes`.
fn reference_encoder() -> Encoder {
    let cbor = cbor::encode_bytes(&make_message(256, "Wolf"));
    Encoder::new("bytes", &cbor, 30, 10).expect("within the bounds")
}

#[test]
fn ur_15_the_reference_multipart_sequence_starts_as_published() {
    let mut e = reference_encoder();
    assert_eq!(e.seq_len(), 9);
    assert!(!e.is_single_part());
    // Part 1 in full, and the first 44 characters of part 2, as published.
    assert_eq!(
        e.next_part(),
        "ur:bytes/1-9/lpadascfadaxcywenbpljkhdcahkadaemejtswhhylkepmykhhtsytsnoyoyaxaedsuttydmmhhpktpmsrjtdkgslpgh"
    );
    let two = e.next_part();
    assert!(
        two.starts_with("ur:bytes/2-9/lpaoascfadaxcywenbpljkhdcagwdpf"),
        "{two}"
    );
}

#[test]
fn ur_15_the_reference_sequence_pins_from_this_code() {
    // Not published values: taken from this code once the parts above and
    // the choices of the vector above matched, to catch a later change
    // (`scripts/ur-crosscheck.sh` finds them equal to Hummingbird's).
    let mut e = reference_encoder();
    let parts: Vec<String> = (0..4).map(|_| e.next_part()).collect();
    assert_eq!(
        parts[1],
        "ur:bytes/2-9/lpaoascfadaxcywenbpljkhdcagwdpfnsboxgwlbaawzuefywkdplrsrjynbvygabwjldapfcsgmghhkhstlrdcxaefz"
    );
    assert_eq!(
        parts[2],
        "ur:bytes/3-9/lpaxascfadaxcywenbpljkhdcahelbknlkuejnbadmssfhfrdpsbiegecpasvssovlgeykssjykklronvsjksopdzmol"
    );
    assert_eq!(
        parts[3],
        "ur:bytes/4-9/lpaaascfadaxcywenbpljkhdcasotkhemthydawydtaxneurlkosgwcekonertkbrlwmplssjtammdplolsbrdzcrtas"
    );
}

#[test]
fn ur_15_the_reference_sequence_decodes_to_the_message() {
    let mut e = reference_encoder();
    let mut d = Decoder::new();
    for _ in 0..9 {
        d.receive(&e.next_part()).expect("a good part");
    }
    let (ty, got) = d.result().expect("nine parts are the message");
    assert_eq!(ty, "bytes");
    assert_eq!(
        cbor::decode_bytes(&got),
        Ok(make_message(256, "Wolf").as_slice())
    );
}

#[test]
fn ur_15_the_first_parts_are_the_fragments_and_later_parts_mix() {
    let mut e = reference_encoder();
    let mut texts = Vec::new();
    for _ in 0..30 {
        texts.push(e.next_part());
    }
    // The same encoder state gives the same text: a second encoder repeats.
    let mut again = reference_encoder();
    for t in &texts {
        assert_eq!(&again.next_part(), t);
    }
    // Parts beyond the ninth are not the same as any of the first nine.
    for t in texts.iter().skip(9) {
        assert!(texts.iter().take(9).all(|f| f != t));
    }
}

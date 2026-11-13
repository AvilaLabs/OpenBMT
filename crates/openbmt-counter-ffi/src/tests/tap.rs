//! TP-01 to TP-50: the payment request as a read-only NFC tag.

use bitcoin::hashes::Hash;
use bitcoin::{Network, PubkeyHash, ScriptBuf};
use openbmt_lightning::bolt11::Checked;

use super::kit::p2wsh;
use crate::pay::{payment_for, payment_with_invoice, Payment};
use crate::tap::{tag_content, tap_answer};

const OK: [u8; 2] = [0x90, 0x00];
const SELECT_APP: [u8; 13] = [
    0x00, 0xA4, 0x04, 0x00, 0x07, 0xD2, 0x76, 0x00, 0x00, 0x85, 0x01, 0x01, 0x00,
];
const SELECT_CC: [u8; 7] = [0x00, 0xA4, 0x00, 0x0C, 0x02, 0xE1, 0x03];
const SELECT_NDEF: [u8; 7] = [0x00, 0xA4, 0x00, 0x0C, 0x02, 0xE1, 0x04];

/// What a minimal reader of NDEF makes of a file: the type name format, the
/// type, and the payload, when the file is one well-formed message of
/// exactly one record. Any other shape is `None`.
fn ndef_read(file: &[u8]) -> Option<(u8, Vec<u8>, Vec<u8>)> {
    let nlen = usize::from(u16::from_be_bytes([*file.first()?, *file.get(1)?]));
    let message = file.get(2..)?;
    if message.len() != nlen {
        return None;
    }
    let flags = *message.first()?;
    let (mb, me, cf, sr, il) = (
        flags & 0x80 != 0,
        flags & 0x40 != 0,
        flags & 0x20 != 0,
        flags & 0x10 != 0,
        flags & 0x08 != 0,
    );
    // One record: the first and the last, not chunked, with no identifier.
    if !(mb && me) || cf || il {
        return None;
    }
    let tnf = flags & 0x07;
    let type_len = usize::from(*message.get(1)?);
    let (payload_len, at) = if sr {
        (usize::from(*message.get(2)?), 3)
    } else {
        let b: [u8; 4] = message.get(2..6)?.try_into().ok()?;
        (usize::try_from(u32::from_be_bytes(b)).ok()?, 6)
    };
    let type_end = at + type_len;
    let payload_end = type_end + payload_len;
    // Nothing is left after the one record.
    if message.len() != payload_end {
        return None;
    }
    Some((
        tnf,
        message.get(at..type_end)?.to_vec(),
        message.get(type_end..payload_end)?.to_vec(),
    ))
}

/// The text of a URI record in a file: the prefix code must be 0.
fn uri_text(file: &[u8]) -> String {
    let (tnf, kind, payload) = ndef_read(file).expect("one record");
    assert_eq!((tnf, kind.as_slice()), (1, b"U".as_slice()));
    assert_eq!(payload.first(), Some(&0), "identifier code 0");
    String::from_utf8(payload.get(1..).expect("text").to_vec()).expect("UTF-8")
}

fn chain(sats: u64) -> Payment {
    payment_for(&p2wsh(1), Network::Bitcoin, sats).expect("a payment")
}

fn invoice(letters: usize, sats: u64) -> Checked {
    Checked {
        bolt11: format!("lnbc1{}", "q".repeat(letters)),
        amount_msat: sats * 1_000,
        payment_hash: [7; 32],
        timestamp: 0,
    }
}

fn lightning(letters: usize, sats: u64) -> Payment {
    payment_with_invoice(&p2wsh(1), Network::Bitcoin, sats, &invoice(letters, sats))
        .expect("a payment")
}

/// One command: the new session and the response.
fn step(session: &mut Vec<u8>, content: Option<&[u8]>, apdu: &[u8]) -> Vec<u8> {
    let (next, response) = tap_answer(session, content, apdu);
    *session = next;
    response
}

fn status(response: &[u8]) -> [u8; 2] {
    let n = response.len();
    [response[n - 2], response[n - 1]]
}

fn read_cmd(offset: u16, le: u8) -> [u8; 5] {
    let [hi, lo] = offset.to_be_bytes();
    [0x00, 0xB0, hi, lo, le]
}

/// The file an Android reader gets: SELECT application, SELECT E103, READ
/// the capability file, SELECT E104, READ the length, READ the rest in
/// chunks of at most `chunk` bytes.
fn read_tag(content: &[u8], chunk: u8) -> Vec<u8> {
    let mut s = Vec::new();
    let c = Some(content);
    assert_eq!(step(&mut s, c, &SELECT_APP), OK);
    assert_eq!(step(&mut s, c, &SELECT_CC), OK);
    let cc = step(&mut s, c, &read_cmd(0, 0x0F));
    assert_eq!(status(&cc), OK);
    assert_eq!(cc.len(), 17);
    assert_eq!(step(&mut s, c, &SELECT_NDEF), OK);
    let nlen = step(&mut s, c, &read_cmd(0, 2));
    assert_eq!(status(&nlen), OK);
    let total = 2 + usize::from(u16::from_be_bytes([nlen[0], nlen[1]]));
    let mut file = nlen[..2].to_vec();
    while file.len() < total {
        let want = u8::try_from((total - file.len()).min(usize::from(chunk))).expect("fits");
        let offset = u16::try_from(file.len()).expect("fits");
        let part = step(&mut s, c, &read_cmd(offset, want));
        assert_eq!(status(&part), OK);
        assert_eq!(part.len(), usize::from(want) + 2);
        file.extend_from_slice(&part[..part.len() - 2]);
    }
    file
}

// ---- what the tag holds (TP-01 to TP-03) ----

#[test]
fn tp_01_the_tag_of_an_on_chain_request_reads_back_as_the_qr_text() {
    for sats in [0, 1, 5_000, 150_000_000] {
        let p = chain(sats);
        assert!(!p.tag.is_empty());
        let text = uri_text(&p.tag);
        assert!(text.eq_ignore_ascii_case(&p.uri), "{text} / {}", p.uri);
        assert_eq!(text, p.copy_uri);
        assert_eq!(text, text.to_lowercase());
    }
}

#[test]
fn tp_01_the_tag_of_a_base58_request_is_the_copy_text_as_it_is() {
    let script = ScriptBuf::new_p2pkh(&PubkeyHash::from_byte_array([3; 20]));
    let p = payment_for(&script, Network::Bitcoin, 700).expect("a payment");
    let text = uri_text(&p.tag);
    assert_eq!(text, p.copy_uri);
    assert!(text.eq_ignore_ascii_case(&p.uri));
    assert!(text.starts_with("bitcoin:1"), "{text}");
}

#[test]
fn tp_01_the_tag_of_a_request_with_an_invoice_reads_back_as_the_qr_text() {
    let p = lightning(120, 5_000);
    let text = uri_text(&p.tag);
    assert!(text.eq_ignore_ascii_case(&p.uri), "{text} / {}", p.uri);
    assert_eq!(text, p.copy_uri);
    assert!(text.contains("&lightning=lnbc1qqq"));
    assert_eq!(text, text.to_lowercase());
}

#[test]
fn tp_02_the_record_is_one_plain_uri_record_with_no_prefix_code() {
    let file = tag_content("bitcoin:bc1qxyz").expect("fits");
    // NLEN, then D1 (MB, ME, SR, TNF 1), type length 1, payload length,
    // type U, identifier code 0 and the text.
    let mut want = vec![0x00, 0x14, 0xD1, 0x01, 0x10, b'U', 0x00];
    want.extend_from_slice(b"bitcoin:bc1qxyz");
    assert_eq!(file, want);
    assert_eq!(ndef_read(&file).expect("one record").0, 1);
    // The message is the one record: nothing after it, no application
    // record, no label, shop name, sale number or counter ID.
    assert_eq!(usize::from(file[1]), file.len() - 2);
    let p = chain(5_000);
    let (_, _, payload) = ndef_read(&p.tag).expect("one record");
    assert_eq!(payload.len(), p.copy_uri.len() + 1);
    assert!(!p.copy_uri.contains("label") && !p.copy_uri.contains("message"));
}

#[test]
fn tp_02_the_record_is_short_up_to_255_bytes_of_payload_and_long_after() {
    // A payload of 255 is the text of 254 and the prefix code.
    let short = tag_content(&"a".repeat(254)).expect("fits");
    assert_eq!(short[2], 0xD1);
    assert_eq!(short[4], 255);
    assert_eq!(uri_text(&short), "a".repeat(254));
    let long = tag_content(&"a".repeat(255)).expect("fits");
    assert_eq!(long[2], 0xC1);
    assert_eq!(&long[4..8], &256u32.to_be_bytes());
    assert_eq!(uri_text(&long), "a".repeat(255));
}

#[test]
fn tp_03_the_longest_message_is_2048_bytes_and_a_longer_one_is_left_off() {
    let fits = tag_content(&"a".repeat(2040)).expect("2048 fits");
    assert_eq!(fits.len(), 2050);
    assert_eq!(usize::from(u16::from_be_bytes([fits[0], fits[1]])), 2048);
    assert_eq!(uri_text(&fits), "a".repeat(2040));
    assert_eq!(tag_content(&"a".repeat(2041)), None);
    assert_eq!(tag_content(&"a".repeat(1 << 20)), None);
}

#[test]
fn tp_03_a_request_that_fits_exactly_has_a_tag_and_one_that_does_not_has_none() {
    // The invoice that makes the text 2040 bytes, and the one that makes it
    // one more.
    let base = lightning(0, 5_000).copy_uri.len();
    let fits = lightning(2040 - base, 5_000);
    assert_eq!(fits.copy_uri.len(), 2040);
    assert_eq!(uri_text(&fits.tag), fits.copy_uri);
    assert_eq!(fits.tag.len(), 2050);
    let over = lightning(2041 - base, 5_000);
    assert_eq!(over.copy_uri.len(), 2041);
    // The QR code is there as before and the tag is empty.
    assert!(over.tag.is_empty());
    assert!(over.qr_size > 0 && !over.uri.is_empty());
}

// ---- no sale, and changes (TP-04, TP-05) ----

#[test]
fn tp_04_with_no_sale_every_command_is_not_found() {
    let mut commands: Vec<Vec<u8>> = vec![
        SELECT_APP.to_vec(),
        SELECT_CC.to_vec(),
        SELECT_NDEF.to_vec(),
        read_cmd(0, 2).to_vec(),
        vec![0x00, 0xD6, 0x00, 0x00, 0x01, 0x00],
        vec![0x00, 0x01, 0x00, 0x00],
        vec![0x80, 0xA4, 0x00, 0x00],
        vec![],
        vec![0xFF; 300],
    ];
    commands.push(vec![0x00; 262]);
    for cmd in commands {
        let (session, response) = tap_answer(&[], None, &cmd);
        assert_eq!(response, vec![0x6A, 0x82], "{cmd:02x?}");
        assert!(session.is_empty());
    }
}

#[test]
fn tp_04_a_content_that_is_no_file_is_not_found_too() {
    for content in [&[][..], &[0u8][..], &vec![0u8; 2051][..]] {
        let (_, response) = tap_answer(&[], Some(content), &SELECT_APP);
        assert_eq!(response, vec![0x6A, 0x82]);
    }
}

#[test]
fn tp_04_a_session_does_not_outlive_its_sale() {
    let content = chain(5_000).tag;
    let mut s = Vec::new();
    assert_eq!(step(&mut s, Some(&content), &SELECT_APP), OK);
    assert_eq!(step(&mut s, Some(&content), &SELECT_NDEF), OK);
    assert!(!s.is_empty());
    // No sale: nothing is answered and the snapshot is dropped, not even
    // the last request is held.
    assert_eq!(step(&mut s, None, &read_cmd(0, 2)), vec![0x6A, 0x82]);
    assert!(s.is_empty());
    // The same session handed back with a sale again reads nothing.
    let kept = {
        let mut k = Vec::new();
        step(&mut k, Some(&content), &SELECT_APP);
        step(&mut k, Some(&content), &SELECT_NDEF);
        k
    };
    let (_, response) = tap_answer(&[], Some(&content), &read_cmd(0, 2));
    assert_eq!(response, vec![0x69, 0x86]);
    // A reader that selected before the end can still read afterwards only
    // with a sale: the snapshot is what it selected.
    let (_, response) = tap_answer(&kept, Some(&content), &read_cmd(0, 2));
    assert_eq!(status(&response), OK);
}

#[test]
fn tp_05_a_reader_gets_the_content_it_selected_and_a_later_selection_the_new_one() {
    let first = chain(5_000).tag;
    let second = chain(2_000).tag;
    assert_ne!(first, second);
    let mut s = Vec::new();
    assert_eq!(step(&mut s, Some(&first), &SELECT_APP), OK);
    assert_eq!(step(&mut s, Some(&first), &SELECT_NDEF), OK);
    // The request changes between two reads: the old file goes on.
    let all = step(&mut s, Some(&second), &read_cmd(0, 200));
    assert_eq!(status(&all), OK);
    assert_eq!(&all[..all.len() - 2], &first[..]);
    // A later selection gets the new content.
    assert_eq!(step(&mut s, Some(&second), &SELECT_NDEF), OK);
    let all = step(&mut s, Some(&second), &read_cmd(0, 200));
    assert_eq!(&all[..all.len() - 2], &second[..]);
}

#[test]
fn tp_05_when_the_sale_ends_the_old_content_is_refused() {
    let content = chain(5_000).tag;
    let mut s = Vec::new();
    step(&mut s, Some(&content), &SELECT_APP);
    step(&mut s, Some(&content), &SELECT_NDEF);
    let half = step(&mut s, Some(&content), &read_cmd(0, 10));
    assert_eq!(status(&half), OK);
    let blob = s.clone();
    // The sale ends: the read of the rest is refused, with the session the
    // app still holds as well as with none.
    for session in [&s[..], &blob[..]] {
        let (_, response) = tap_answer(session, None, &read_cmd(10, 20));
        assert_eq!(response, vec![0x6A, 0x82]);
    }
    // A sale that starts later is not read through the old selection.
    let (after, response) = tap_answer(&[], Some(&chain(9).tag), &read_cmd(10, 20));
    assert_eq!(response, vec![0x69, 0x86]);
    assert!(after.is_empty());
}

// ---- the commands (TP-10 to TP-15) ----

#[test]
fn tp_10_the_application_is_selected_by_its_id_and_no_other() {
    let c = chain(1).tag;
    let mut s = Vec::new();
    assert_eq!(step(&mut s, Some(&c), &SELECT_APP), OK);
    assert_eq!(step(&mut s, Some(&c), &SELECT_APP[..11]), vec![0x67, 0x00]);
    // Another ID: not found.
    let mut other = SELECT_APP;
    other[11] = 0x02;
    assert_eq!(step(&mut s, Some(&c), &other), vec![0x6A, 0x82]);
    // Without the trailing Le, and with P2 0C, it is the same.
    let mut s = Vec::new();
    assert_eq!(step(&mut s, Some(&c), &SELECT_APP[..12]), OK);
    let mut quiet = SELECT_APP[..12].to_vec();
    quiet[3] = 0x0C;
    assert_eq!(step(&mut s, Some(&c), &quiet), OK);
    // A wrong length of the ID and a missing ID.
    let short = [
        0x00, 0xA4, 0x04, 0x00, 0x06, 0xD2, 0x76, 0x00, 0x00, 0x85, 0x01,
    ];
    assert_eq!(step(&mut s, Some(&c), &short), vec![0x67, 0x00]);
    assert_eq!(
        step(&mut s, Some(&c), &[0x00, 0xA4, 0x04, 0x00]),
        vec![0x67, 0x00]
    );
    // A P1 or P2 that is not offered.
    assert_eq!(
        step(
            &mut s,
            Some(&c),
            &[0x00, 0xA4, 0x08, 0x00, 0x02, 0xE1, 0x03]
        ),
        vec![0x6A, 0x86]
    );
    assert_eq!(
        step(
            &mut s,
            Some(&c),
            &[0x00, 0xA4, 0x00, 0x04, 0x02, 0xE1, 0x03]
        ),
        vec![0x6A, 0x86]
    );
}

#[test]
fn tp_11_a_file_is_selected_by_id_after_the_application_and_only_e103_or_e104() {
    let c = chain(1).tag;
    let mut s = Vec::new();
    // Before the application is selected there is no file to select.
    assert_eq!(step(&mut s, Some(&c), &SELECT_CC), vec![0x6A, 0x82]);
    assert_eq!(step(&mut s, Some(&c), &SELECT_NDEF), vec![0x6A, 0x82]);
    assert_eq!(step(&mut s, Some(&c), &SELECT_APP), OK);
    assert_eq!(step(&mut s, Some(&c), &SELECT_CC), OK);
    assert_eq!(step(&mut s, Some(&c), &SELECT_NDEF), OK);
    // Any other file is not found, and the selection stays.
    assert_eq!(
        step(
            &mut s,
            Some(&c),
            &[0x00, 0xA4, 0x00, 0x0C, 0x02, 0x3F, 0x00]
        ),
        vec![0x6A, 0x82]
    );
    assert_eq!(status(&step(&mut s, Some(&c), &read_cmd(0, 2))), OK);
    // An ID of the wrong length.
    assert_eq!(
        step(&mut s, Some(&c), &[0x00, 0xA4, 0x00, 0x0C, 0x01, 0xE1]),
        vec![0x67, 0x00]
    );
}

#[test]
fn tp_11_other_instructions_and_classes_and_writing_are_refused() {
    let c = chain(1).tag;
    let mut s = Vec::new();
    step(&mut s, Some(&c), &SELECT_APP);
    step(&mut s, Some(&c), &SELECT_NDEF);
    for ins in [0x00u8, 0x01, 0xA0, 0xA5, 0xB1, 0xC0, 0xCA, 0xD0, 0xD7, 0xFF] {
        assert_eq!(
            step(&mut s, Some(&c), &[0x00, ins, 0x00, 0x00]),
            vec![0x6D, 0x00],
            "ins {ins:02x}"
        );
    }
    for cla in [0x01u8, 0x04, 0x0C, 0x10, 0x80, 0x84, 0x90, 0xFF] {
        assert_eq!(
            step(&mut s, Some(&c), &[cla, 0xB0, 0x00, 0x00, 0x02]),
            vec![0x6E, 0x00],
            "cla {cla:02x}"
        );
    }
    // UPDATE BINARY is refused, with data, for the capability file and the
    // NDEF file, and changes nothing the reader can read.
    let write = [0x00, 0xD6, 0x00, 0x00, 0x02, 0x00, 0x00];
    assert_eq!(step(&mut s, Some(&c), &write), vec![0x69, 0x82]);
    step(&mut s, Some(&c), &SELECT_CC);
    assert_eq!(step(&mut s, Some(&c), &write), vec![0x69, 0x82]);
    let mut fresh = Vec::new();
    assert_eq!(step(&mut fresh, Some(&c), &write), vec![0x69, 0x82]);
    assert_eq!(read_tag(&c, 255).len(), c.len());
}

#[test]
fn tp_12_the_capability_file_says_what_the_tag_does() {
    let c = chain(1).tag;
    let mut s = Vec::new();
    step(&mut s, Some(&c), &SELECT_APP);
    step(&mut s, Some(&c), &SELECT_CC);
    let r = step(&mut s, Some(&c), &read_cmd(0, 0x0F));
    assert_eq!(
        r,
        vec![
            0x00, 0x0F, // CCLEN
            0x20, // mapping version 2.0
            0x00, 0xFF, // MLe: 255 bytes a read
            0x00, 0x01, // MLc
            0x04, 0x06, // NDEF file control TLV
            0xE1, 0x04, // file ID
            0x08, 0x02, // at most 2050 bytes
            0x00, // read access
            0xFF, // write access: none
            0x90, 0x00
        ]
    );
    // The same file whatever the content is.
    let other = tag_content(&"b".repeat(2040)).expect("fits");
    let mut s = Vec::new();
    step(&mut s, Some(&other), &SELECT_APP);
    step(&mut s, Some(&other), &SELECT_CC);
    assert_eq!(step(&mut s, Some(&other), &read_cmd(0, 0x0F)), r);
}

#[test]
fn tp_13_reads_are_refused_until_a_file_is_selected_and_after_the_application_again() {
    let c = chain(1).tag;
    let mut s = Vec::new();
    assert_eq!(step(&mut s, Some(&c), &read_cmd(0, 2)), vec![0x69, 0x86]);
    step(&mut s, Some(&c), &SELECT_APP);
    assert_eq!(step(&mut s, Some(&c), &read_cmd(0, 2)), vec![0x69, 0x86]);
    step(&mut s, Some(&c), &SELECT_NDEF);
    assert_eq!(status(&step(&mut s, Some(&c), &read_cmd(0, 2))), OK);
    // The application again: the file is forgotten.
    step(&mut s, Some(&c), &SELECT_APP);
    assert_eq!(step(&mut s, Some(&c), &read_cmd(0, 2)), vec![0x69, 0x86]);
    step(&mut s, Some(&c), &SELECT_CC);
    assert_eq!(status(&step(&mut s, Some(&c), &read_cmd(0, 2))), OK);
    step(&mut s, Some(&c), &SELECT_APP);
    assert_eq!(step(&mut s, Some(&c), &read_cmd(0, 2)), vec![0x69, 0x86]);
}

#[test]
fn tp_13_a_read_beyond_the_file_is_refused_and_one_past_the_end_stops_at_it() {
    let c = chain(5_000).tag;
    let n = u16::try_from(c.len()).expect("small");
    let mut s = Vec::new();
    step(&mut s, Some(&c), &SELECT_APP);
    step(&mut s, Some(&c), &SELECT_NDEF);
    assert_eq!(
        step(&mut s, Some(&c), &read_cmd(n + 1, 2)),
        vec![0x6B, 0x00]
    );
    assert_eq!(
        step(&mut s, Some(&c), &read_cmd(0x7FFF, 2)),
        vec![0x6B, 0x00]
    );
    // At the end there is nothing more to give.
    assert_eq!(step(&mut s, Some(&c), &read_cmd(n, 2)), OK);
    // A read that runs past the end answers up to it.
    let tail = step(&mut s, Some(&c), &read_cmd(n - 3, 100));
    assert_eq!(&tail[..3], &c[usize::from(n) - 3..]);
    assert_eq!(status(&tail), OK);
    assert_eq!(tail.len(), 5);
    // The top bit of P1 names a file by short ID, which is not offered.
    assert_eq!(
        step(&mut s, Some(&c), &[0x00, 0xB0, 0x81, 0x00, 0x02]),
        vec![0x6A, 0x86]
    );
}

#[test]
fn tp_13_a_read_never_answers_more_than_255_bytes() {
    let c = tag_content(&"a".repeat(2040)).expect("fits");
    let mut s = Vec::new();
    step(&mut s, Some(&c), &SELECT_APP);
    step(&mut s, Some(&c), &SELECT_NDEF);
    // Le 255, Le 0 (256) and nothing else is more than 255 bytes.
    for le in [255u8, 0, 128, 1] {
        let r = step(&mut s, Some(&c), &read_cmd(0, le));
        assert_eq!(status(&r), OK);
        let want = if le == 0 { 255 } else { usize::from(le) };
        assert_eq!(r.len(), want + 2, "le {le}");
        assert_eq!(&r[..r.len() - 2], &c[..want]);
    }
}

#[test]
fn tp_14_commands_whose_lengths_do_not_agree_are_refused_and_change_nothing() {
    let c = chain(5_000).tag;
    let mut s = Vec::new();
    step(&mut s, Some(&c), &SELECT_APP);
    step(&mut s, Some(&c), &SELECT_NDEF);
    let before = s.clone();
    let mut commands: Vec<Vec<u8>> = vec![
        vec![],
        vec![0x00],
        vec![0x00, 0xB0, 0x00],
        // Lc 7 with 6 bytes, and with 9.
        SELECT_APP[..11].to_vec(),
        vec![
            0x00, 0xA4, 0x04, 0x00, 0x07, 0xD2, 0x76, 0x00, 0x00, 0x85, 0x01, 0x01, 0x00, 0x00,
        ],
        // Lc 0 with a byte after it, and extended lengths.
        vec![0x00, 0xB0, 0x00, 0x00, 0x00, 0x00],
        vec![0x00, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x02],
        vec![0x00, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x00],
        vec![0x00, 0xA4, 0x04, 0x00, 0x00, 0x00, 0x07, 0xD2, 0x76],
        // A read with data, and a read with no Le.
        vec![0x00, 0xB0, 0x00, 0x00, 0x01, 0x00],
        vec![0x00, 0xB0, 0x00, 0x00],
    ];
    // Longer than 261 bytes, even when the first part is a good command.
    let mut long = SELECT_APP.to_vec();
    long.resize(262, 0);
    commands.push(long);
    commands.push(vec![0xFF; 262]);
    commands.push(vec![0x00; 10_000]);
    for cmd in commands {
        let (after, response) = tap_answer(&s, Some(&c), &cmd);
        assert_eq!(response, vec![0x67, 0x00], "{} bytes", cmd.len());
        assert_eq!(after, before);
    }
    // The largest command that is parsed is 261 bytes: 255 bytes of data.
    let mut big = vec![0x00, 0xA4, 0x04, 0x00, 0xFF];
    big.resize(260, 0);
    big.push(0x00);
    assert_eq!(big.len(), 261);
    assert_eq!(tap_answer(&s, Some(&c), &big).1, vec![0x67, 0x00]);
    // Still on the file it had.
    assert_eq!(status(&step(&mut s, Some(&c), &read_cmd(0, 2))), OK);
}

#[test]
fn tp_15_the_selection_and_the_content_are_dropped_with_the_session() {
    let c = chain(5_000).tag;
    let mut s = Vec::new();
    step(&mut s, Some(&c), &SELECT_APP);
    step(&mut s, Some(&c), &SELECT_NDEF);
    assert!(!s.is_empty());
    // The reader goes away: the app passes the empty session again.
    assert_eq!(
        step(&mut Vec::new(), Some(&c), &read_cmd(0, 2)),
        vec![0x69, 0x86]
    );
}

#[test]
fn tp_14_a_session_of_another_shape_is_the_start_state() {
    let c = chain(5_000).tag;
    let mut good = Vec::new();
    step(&mut good, Some(&c), &SELECT_APP);
    step(&mut good, Some(&c), &SELECT_NDEF);
    let mut cut = good.clone();
    cut.pop();
    let mut extra = good.clone();
    extra.push(0);
    let mut wrong_len = good.clone();
    wrong_len[2] ^= 1;
    let mut huge = vec![3, 0x10, 0x00];
    huge.resize(3 + 4096, 0);
    for bad in [
        vec![0],
        vec![4],
        vec![1, 0],
        vec![2, 2],
        vec![3],
        vec![3, 0, 0, 1],
        vec![0xFF; 40],
        cut,
        extra,
        wrong_len,
        huge,
    ] {
        let (_, response) = tap_answer(&bad, Some(&c), &read_cmd(0, 2));
        assert_eq!(response, vec![0x69, 0x86], "{bad:02x?}");
        // And the tap goes on from the start.
        let (next, response) = tap_answer(&bad, Some(&c), &SELECT_APP);
        assert_eq!(response, OK);
        assert_eq!(next, vec![1]);
    }
    // The session that was kept is read as it was.
    assert_eq!(status(&tap_answer(&good, Some(&c), &read_cmd(0, 2)).1), OK);
}

// ---- a whole read (TP-50) ----

#[test]
fn tp_50_a_reader_gets_the_whole_file_in_chunks_of_every_size() {
    let forms = [
        chain(0).tag,
        chain(5_000).tag,
        lightning(120, 5_000).tag,
        tag_content(&"a".repeat(254)).expect("fits"),
        tag_content(&"a".repeat(255)).expect("fits"),
        tag_content(&"a".repeat(2040)).expect("fits"),
    ];
    for content in forms {
        for chunk in [255u8, 254, 100, 16, 1] {
            if chunk == 1 && content.len() > 600 {
                continue;
            }
            assert_eq!(read_tag(&content, chunk), content, "chunk {chunk}");
        }
    }
}

#[test]
fn tp_50_the_file_a_reader_gets_is_the_qr_text_for_every_form() {
    for p in [chain(0), chain(5_000), lightning(120, 5_000)] {
        let file = read_tag(&p.tag, 255);
        assert!(uri_text(&file).eq_ignore_ascii_case(&p.uri));
    }
}

#[test]
fn tp_14_no_command_makes_the_answer_panic_or_run_long() {
    let c = tag_content(&"a".repeat(300)).expect("fits");
    let mut seed = 0x1234_5678_9ABC_DEF0u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut session = Vec::new();
    for _ in 0..20_000 {
        let len = (next() % 270) as usize;
        let mut apdu: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        // Make a good part of them look like the real ones.
        if len >= 4 && next() % 2 == 0 {
            apdu[0] = 0;
            apdu[1] = [0xA4, 0xB0, 0xD6][(next() % 3) as usize];
        }
        let content = if next() % 8 == 0 { None } else { Some(&c[..]) };
        let (after, response) = tap_answer(&session, content, &apdu);
        assert!(response.len() >= 2 && response.len() <= 255 + 2);
        assert!(after.len() <= 3 + 2050);
        session = after;
        if next() % 16 == 0 {
            session.clear();
        }
    }
}

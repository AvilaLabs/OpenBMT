//! The second call: every operation, every refusal and the strict framing.

use openbmt_qr::{encode_with, Ecc};
use openbmt_ur::bytewords::encode_minimal;
use openbmt_ur::cbor::{decode_bytes, encode_bytes};
use openbmt_ur::hash::crc32;
use openbmt_ur::{Decoder, Encoder};

use crate::call::*;

// ---- building requests and reading replies ----------------------------------

fn head(op: u8) -> Vec<u8> {
    let mut v = REQUEST_MAGIC.to_vec();
    v.push(VERSION);
    v.push(op);
    v
}

fn qr_request(level: u8, text: &[u8]) -> Vec<u8> {
    let mut v = head(OP_QR_ENCODE);
    v.push(level);
    v.extend_from_slice(&(text.len() as u32).to_be_bytes());
    v.extend_from_slice(text);
    v
}

fn ur_request(ur_type: &str, max_fragment: u16, payload: &[u8]) -> Vec<u8> {
    let mut v = head(OP_UR_ENCODE);
    v.push(ur_type.len() as u8);
    v.extend_from_slice(ur_type.as_bytes());
    v.extend_from_slice(&max_fragment.to_be_bytes());
    v.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

fn key_request(ur_type: &str, message: &[u8]) -> Vec<u8> {
    let mut v = head(OP_KEY_TEXT);
    v.push(ur_type.len() as u8);
    v.extend_from_slice(ur_type.as_bytes());
    v.extend_from_slice(&(message.len() as u32).to_be_bytes());
    v.extend_from_slice(message);
    v
}

fn new_request() -> Vec<u8> {
    head(OP_DECODER_NEW)
}

fn receive_request(handle: u32, text: &[u8]) -> Vec<u8> {
    let mut v = head(OP_DECODER_RECEIVE);
    v.extend_from_slice(&handle.to_be_bytes());
    v.extend_from_slice(&(text.len() as u32).to_be_bytes());
    v.extend_from_slice(text);
    v
}

fn handle_request(op: u8, handle: u32) -> Vec<u8> {
    let mut v = head(op);
    v.extend_from_slice(&handle.to_be_bytes());
    v
}

/// The body of an ok reply, or the code of a refusal, with the framing of the
/// reply checked strictly.
fn parse(reply: &[u8]) -> Result<Vec<u8>, String> {
    assert_eq!(reply.get(..4), Some(REPLY_MAGIC.as_slice()));
    assert_eq!(reply.get(4), Some(&VERSION));
    let body = reply.get(6..).expect("a status");
    match reply.get(5) {
        Some(&OK) => Ok(body.to_vec()),
        Some(&REFUSED) => {
            assert_eq!(body.len(), 5, "a rule code is five bytes");
            let code = String::from_utf8(body.to_vec()).expect("ascii");
            assert!(code.starts_with("UR-") || code.starts_with("QR-"), "{code}");
            Err(code)
        }
        Some(&REFUSED_WITH_WORDS) => Err(words_of(reply).0),
        other => panic!("status {other:?}"),
    }
}

/// The rule code and the three texts of a reply that has words, with the
/// framing checked strictly.
fn words_of(reply: &[u8]) -> (String, [String; 3]) {
    assert_eq!(reply.get(..4), Some(REPLY_MAGIC.as_slice()));
    assert_eq!(reply.get(4), Some(&VERSION));
    assert_eq!(reply.get(5), Some(&REFUSED_WITH_WORDS));
    let body = reply.get(6..).expect("a body");
    let code = String::from_utf8(body[..5].to_vec()).expect("ascii");
    assert!(code.starts_with("UR-"), "{code}");
    let mut at = 5;
    let mut texts = Vec::new();
    for _ in 0..3 {
        let len = u16::from_be_bytes([body[at], body[at + 1]]) as usize;
        at += 2;
        texts.push(String::from_utf8(body[at..at + len].to_vec()).expect("UTF-8"));
        at += len;
    }
    assert_eq!(at, body.len(), "nothing after the third text");
    (code, texts.try_into().expect("three"))
}

fn refused(reply: &[u8]) -> String {
    parse(reply).expect_err("a refusal")
}

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_be_bytes(b[at..at + 4].try_into().expect("four")) as usize
}

/// The parts of a `ur_encode` body.
fn parts(body: &[u8]) -> Vec<String> {
    let n = u32_at(body, 0);
    let mut at = 4;
    let mut out = Vec::new();
    for _ in 0..n {
        let len = u32_at(body, at);
        at += 4;
        out.push(String::from_utf8(body[at..at + len].to_vec()).expect("text"));
        at += len;
    }
    assert_eq!(at, body.len(), "nothing after the last part");
    out
}

fn message(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 7 + 3) as u8).collect()
}

fn open(calls: &Calls) -> u32 {
    let body = parse(&calls.call(&new_request())).expect("a handle");
    assert_eq!(body.len(), 4);
    u32_at(&body, 0) as u32
}

/// The part text of a multi-part body made by hand.
fn part_text(
    ur_type: &str,
    seq: u8,
    seq_len: u8,
    message_len: u8,
    checksum: u32,
    data: &[u8],
) -> String {
    let mut body = vec![0x85, seq, seq_len, message_len, 0x1a];
    body.extend_from_slice(&checksum.to_be_bytes());
    body.extend_from_slice(&encode_bytes(data));
    format!("ur:{ur_type}/{seq}-{seq_len}/{}", encode_minimal(&body))
}

fn whole(ur_type: &str, cbor: &[u8]) -> String {
    format!("ur:{ur_type}/{}", encode_minimal(cbor))
}

// ---- qr_encode ----------------------------------------------------------------

#[test]
fn qr_jni_call_qr_encode_gives_the_modules_of_the_encoder() {
    let calls = Calls::new();
    for (level, ecc) in [(0, Ecc::L), (1, Ecc::M), (2, Ecc::Q), (3, Ecc::H)] {
        for text in [
            &b"HELLO WORLD"[..],
            b"12345678901234567890",
            b"mixed Case \xc3\xa9 text with more bytes than version one holds",
            b"",
        ] {
            let body = parse(&calls.call(&qr_request(level, text))).expect("a code");
            let code = encode_with(text, ecc, None, None).expect("fits");
            let side = code.size();
            assert_eq!(u16::from_be_bytes([body[0], body[1]]) as usize, side);
            assert_eq!(body.len(), 2 + (side * side).div_ceil(8));
            for y in 0..side {
                for x in 0..side {
                    let i = y * side + x;
                    let bit = (body[2 + i / 8] >> (7 - i % 8)) & 1 == 1;
                    assert_eq!(bit, code.get(x, y), "level {level} module {x},{y}");
                }
            }
            // The unused bits of the last byte are zero.
            let used = (side * side) % 8;
            if used != 0 {
                assert_eq!(body.last().copied().unwrap_or(0) << used, 0);
            }
        }
    }
}

#[test]
fn qr_jni_call_qr_encode_refuses_what_does_not_fit_and_a_bad_level() {
    let calls = Calls::new();
    // Too long for version 40 at level H: QR-01.
    assert_eq!(
        refused(&calls.call(&qr_request(3, &vec![b'a'; 3000]))),
        "QR-01"
    );
    assert_eq!(refused(&calls.call(&qr_request(4, b"x"))), BAD_FRAMING);
    assert_eq!(refused(&calls.call(&qr_request(255, b"x"))), BAD_FRAMING);
}

// ---- ur_encode -----------------------------------------------------------------

#[test]
fn qr_jni_call_ur_encode_a_short_message_is_one_upper_case_part() {
    let calls = Calls::new();
    let payload = message(40);
    let p = parts(&parse(&calls.call(&ur_request("bytes", 230, &payload))).expect("parts"));
    assert_eq!(p.len(), 1);
    assert!(p[0].starts_with("UR:BYTES/") && p[0] == p[0].to_ascii_uppercase());
    let mut d = Decoder::new();
    assert!(d.receive(&p[0]).expect("part").complete);
    let (t, cbor) = d.result().expect("done");
    assert_eq!(t, "bytes");
    assert_eq!(decode_bytes(&cbor).expect("bytes"), payload.as_slice());
}

#[test]
fn qr_jni_call_ur_encode_a_long_message_is_two_turns_of_the_sequence() {
    let calls = Calls::new();
    let payload = message(2000);
    let p = parts(&parse(&calls.call(&ur_request("bytes", 100, &payload))).expect("parts"));
    let mut reference = Encoder::new("bytes", &encode_bytes(&payload), 100, 10).expect("encoder");
    let seq_len = reference.seq_len();
    assert_eq!(p.len(), 2 * seq_len);
    for (i, part) in p.iter().enumerate() {
        assert_eq!(*part, reference.next_part_upper(), "part {i}");
    }
    // The first turn alone completes the message.
    let mut d = Decoder::new();
    for part in p.iter().take(seq_len) {
        d.receive(part).expect("part");
    }
    assert!(d.is_complete());
}

#[test]
fn qr_jni_call_ur_encode_crypto_psbt_needs_a_psbt() {
    let calls = Calls::new();
    let mut psbt = b"psbt\xff".to_vec();
    psbt.extend(message(300));
    let p = parts(&parse(&calls.call(&ur_request("crypto-psbt", 120, &psbt))).expect("parts"));
    assert!(p[0].starts_with("UR:CRYPTO-PSBT/1-"));
    assert_eq!(
        refused(&calls.call(&ur_request("crypto-psbt", 120, b"not a psbt at all"))),
        "UR-14"
    );
}

#[test]
fn qr_jni_call_ur_encode_refusals() {
    let calls = Calls::new();
    let ok = message(50);
    // UR-01: a type that is not 1 to 64 characters of a-z, 0-9, -.
    assert_eq!(
        refused(&calls.call(&ur_request("Bytes", 100, &ok))),
        "UR-01"
    );
    assert_eq!(refused(&calls.call(&ur_request("", 100, &ok))), "UR-01");
    // UR-20: a type the apps do not send.
    assert_eq!(
        refused(&calls.call(&ur_request("psbt", 100, &ok))),
        TYPE_NOT_USED
    );
    assert_eq!(
        refused(&calls.call(&ur_request("foo", 100, &ok))),
        TYPE_NOT_USED
    );
    // UR-02: an empty message, and one over 1 MiB.
    assert_eq!(
        refused(&calls.call(&ur_request("bytes", 100, b""))),
        "UR-02"
    );
    assert_eq!(
        refused(&calls.call(&ur_request("bytes", 2000, &vec![1u8; 1024 * 1024 + 1]))),
        "UR-02"
    );
    // UR-04: a fragment outside 10 to 2000.
    assert_eq!(refused(&calls.call(&ur_request("bytes", 9, &ok))), "UR-04");
    assert_eq!(
        refused(&calls.call(&ur_request("bytes", 2001, &ok))),
        "UR-04"
    );
    // UR-03: more than 10,000 fragments (a megabyte in fragments of 100).
    assert_eq!(
        refused(&calls.call(&ur_request("bytes", 100, &vec![1u8; 1_048_570]))),
        "UR-03"
    );
    // A type that is not UTF-8 is a framing fault.
    let mut v = head(OP_UR_ENCODE);
    v.extend_from_slice(&[2, 0xff, 0xfe, 0, 100, 0, 0, 0, 1, 7]);
    assert_eq!(refused(&calls.call(&v)), BAD_FRAMING);
}

#[test]
fn ur_20_the_apps_use_bytes_and_crypto_psbt_only() {
    let calls = Calls::new();
    let mut psbt = b"psbt\xff".to_vec();
    psbt.extend(message(12));
    assert!(parse(&calls.call(&ur_request("bytes", 100, &message(12)))).is_ok());
    assert!(parse(&calls.call(&ur_request("crypto-psbt", 100, &psbt))).is_ok());
    for other in ["psbt", "foo", "crypto-account", "crypto-hdkey", "x"] {
        assert_eq!(
            refused(&calls.call(&ur_request(other, 100, &psbt))),
            TYPE_NOT_USED,
            "{other}"
        );
    }
    // A finished UR of another type gives no message.
    let h = open(&calls);
    let text = whole("crypto-output", &encode_bytes(b"abcdef"));
    parse(&calls.call(&receive_request(h, text.as_bytes()))).expect("complete");
    assert_eq!(
        refused(&calls.call(&handle_request(OP_DECODER_RESULT, h))),
        TYPE_NOT_USED
    );
}

// ---- the decoders ----------------------------------------------------------------

#[test]
fn qr_jni_call_a_sequence_goes_through_a_decoder_to_its_payload() {
    let calls = Calls::new();
    let payload = message(900);
    let all = parts(&parse(&calls.call(&ur_request("bytes", 90, &payload))).expect("parts"));
    let h = open(&calls);
    assert_eq!(calls.open_decoders(), 1);
    // Nothing yet to take.
    assert_eq!(
        refused(&calls.call(&handle_request(OP_DECODER_RESULT, h))),
        BAD_STATE
    );
    let mut last = Vec::new();
    let mut previous = 0;
    for (i, part) in all.iter().enumerate() {
        // Upper case as shown, and lower case as some senders write.
        let text = if i % 2 == 0 {
            part.clone()
        } else {
            part.to_ascii_lowercase()
        };
        last = parse(&calls.call(&receive_request(h, text.as_bytes()))).expect("progress");
        assert_eq!(last.len(), 13);
        let received = u32_at(&last, 1);
        assert!(received >= previous, "received counts up");
        previous = received;
        assert_eq!(u32_at(&last, 5), all.len() / 2, "the sequence length");
        if last[0] == 1 {
            break;
        }
        assert!(u32_at(&last, 9) < 1_000_000);
    }
    assert_eq!(last[0], 1, "complete");
    assert_eq!(u32_at(&last, 9), 1_000_000);
    let body = parse(&calls.call(&handle_request(OP_DECODER_RESULT, h))).expect("result");
    assert_eq!(body[0] as usize, 5);
    assert_eq!(&body[1..6], b"bytes");
    assert_eq!(u32_at(&body, 6), payload.len());
    assert_eq!(&body[10..], payload.as_slice());
    // The result stays until the decoder is dropped.
    assert!(parse(&calls.call(&handle_request(OP_DECODER_RESULT, h))).is_ok());
    assert!(parse(&calls.call(&handle_request(OP_DECODER_DROP, h)))
        .expect("dropped")
        .is_empty());
    assert_eq!(calls.open_decoders(), 0);
}

#[test]
fn qr_jni_call_a_whole_ur_and_the_psbt_types_come_back_as_their_bytes() {
    let calls = Calls::new();
    let mut psbt = b"psbt\xff".to_vec();
    psbt.extend(message(20));
    for ur_type in ["crypto-psbt", "psbt"] {
        let text = whole(ur_type, &encode_bytes(&psbt));
        let h = open(&calls);
        let p = parse(&calls.call(&receive_request(h, text.as_bytes()))).expect("progress");
        assert_eq!((p[0], u32_at(&p, 1), u32_at(&p, 5)), (1, 1, 1));
        let body = parse(&calls.call(&handle_request(OP_DECODER_RESULT, h))).expect("result");
        assert_eq!(body[0] as usize, ur_type.len());
        assert_eq!(&body[1..1 + ur_type.len()], ur_type.as_bytes());
        assert_eq!(u32_at(&body, 1 + ur_type.len()), psbt.len());
        assert_eq!(&body[5 + ur_type.len()..], psbt.as_slice());
        calls.call(&handle_request(OP_DECODER_DROP, h));
    }
}

#[test]
fn qr_jni_call_a_result_of_a_type_the_apps_do_not_use_is_refused() {
    let calls = Calls::new();
    let h = open(&calls);
    let text = whole("foo", &encode_bytes(b"abcdef"));
    parse(&calls.call(&receive_request(h, text.as_bytes()))).expect("complete");
    assert_eq!(
        refused(&calls.call(&handle_request(OP_DECODER_RESULT, h))),
        TYPE_NOT_USED
    );
    // A PSBT type whose bytes are not a PSBT: UR-14. A byte string that is
    // cut: UR-13.
    let h2 = open(&calls);
    let bad = whole("psbt", &encode_bytes(b"abcdef"));
    parse(&calls.call(&receive_request(h2, bad.as_bytes()))).expect("complete");
    assert_eq!(
        refused(&calls.call(&handle_request(OP_DECODER_RESULT, h2))),
        "UR-14"
    );
    let h3 = open(&calls);
    let cut = whole("bytes", &[0x45, 1, 2]);
    parse(&calls.call(&receive_request(h3, cut.as_bytes()))).expect("complete");
    assert_eq!(
        refused(&calls.call(&handle_request(OP_DECODER_RESULT, h3))),
        "UR-13"
    );
}

#[test]
fn qr_jni_call_a_decoder_refusal_carries_the_rule_and_changes_nothing() {
    let calls = Calls::new();
    let h = open(&calls);
    for (text, code) in [
        (&b"hello"[..], "UR-07"),
        (b"ur:bytes", "UR-07"),
        (b"ur:bytes/zzzzzz", "UR-05"),
        (b"\xff\xfe", "UR-07"),
        (b"", "UR-07"),
    ] {
        assert_eq!(
            refused(&calls.call(&receive_request(h, text))),
            code,
            "{text:?}"
        );
    }
    // A checksum that is wrong.
    let mut good = whole("bytes", &encode_bytes(b"abcdef"));
    let last = good.pop().expect("a letter");
    good.push(if last == 'a' { 'b' } else { 'a' });
    assert!(refused(&calls.call(&receive_request(h, good.as_bytes()))).starts_with("UR-0"));
    // The decoder still works.
    let text = whole("bytes", &encode_bytes(b"abcdef"));
    assert_eq!(
        parse(&calls.call(&receive_request(h, text.as_bytes()))).expect("ok")[0],
        1
    );
}

#[test]
fn qr_jni_call_a_message_that_fails_its_checksum_refuses_with_ur_12_and_starts_again() {
    let calls = Calls::new();
    let message = message(15);
    let a = message[..10].to_vec();
    let mut b = message[10..].to_vec();
    b.resize(10, 0);
    let wrong = crc32(&message) ^ 1;
    let h = open(&calls);
    let first = part_text("bytes", 1, 2, 15, wrong, &a);
    parse(&calls.call(&receive_request(h, first.as_bytes()))).expect("first");
    let second = part_text("bytes", 2, 2, 15, wrong, &b);
    assert_eq!(
        refused(&calls.call(&receive_request(h, second.as_bytes()))),
        "UR-12"
    );
    // Started again from nothing: a good stream completes.
    let good = crc32(&message);
    let first = part_text("bytes", 1, 2, 15, good, &a);
    let second = part_text("bytes", 2, 2, 15, good, &b);
    parse(&calls.call(&receive_request(h, first.as_bytes()))).expect("first");
    let done = parse(&calls.call(&receive_request(h, second.as_bytes()))).expect("second");
    assert_eq!(done[0], 1);
}

#[test]
fn qr_jni_call_a_part_that_disagrees_is_refused_with_ur_10() {
    let calls = Calls::new();
    let a = parts(&parse(&calls.call(&ur_request("bytes", 50, &message(300)))).expect("a"));
    let b = parts(&parse(&calls.call(&ur_request("bytes", 50, &message(301)))).expect("b"));
    let h = open(&calls);
    parse(&calls.call(&receive_request(h, a[0].as_bytes()))).expect("first");
    assert_eq!(
        refused(&calls.call(&receive_request(h, b[1].as_bytes()))),
        "UR-10"
    );
    let p = parse(&calls.call(&receive_request(h, a[1].as_bytes()))).expect("goes on");
    assert_eq!(u32_at(&p, 1), 2);
}

#[test]
fn ur_22_handles_are_at_most_four_never_reused_and_checked() {
    let calls = Calls::new();
    let handles: Vec<u32> = (0..MAX_DECODERS).map(|_| open(&calls)).collect();
    assert_eq!(handles, [1, 2, 3, 4]);
    // The fifth is refused and opens nothing.
    assert_eq!(refused(&calls.call(&new_request())), BAD_STATE);
    assert_eq!(calls.open_decoders(), 4);
    // Dropping one makes room, with a handle never given before.
    parse(&calls.call(&handle_request(OP_DECODER_DROP, 2))).expect("dropped");
    let fresh = open(&calls);
    assert_eq!(fresh, 5);
    assert!(!handles.contains(&fresh));
    // A dropped handle, an unknown one and zero are refused by every operation.
    for stale in [2u32, 0, 99, u32::MAX] {
        assert_eq!(
            refused(&calls.call(&receive_request(stale, b"ur:bytes/x"))),
            BAD_STATE
        );
        assert_eq!(
            refused(&calls.call(&handle_request(OP_DECODER_RESULT, stale))),
            BAD_STATE
        );
        assert_eq!(
            refused(&calls.call(&handle_request(OP_DECODER_DROP, stale))),
            BAD_STATE
        );
    }
    // A decoder is its own: a part given to one is not seen by another.
    let text = whole("bytes", &encode_bytes(b"abcdef"));
    parse(&calls.call(&receive_request(1, text.as_bytes()))).expect("complete");
    assert_eq!(
        refused(&calls.call(&handle_request(OP_DECODER_RESULT, 3))),
        BAD_STATE
    );
    assert!(parse(&calls.call(&handle_request(OP_DECODER_RESULT, 1))).is_ok());
}

#[test]
fn ur_22_a_decoder_holds_no_more_than_the_ur_bounds() {
    // A text over the longest UR is refused before it is read (UR-16), and a
    // request over the call's own limit is refused as framing.
    let calls = Calls::new();
    let h = open(&calls);
    let long = vec![b'a'; openbmt_ur::MAX_TEXT_LEN + 1];
    assert_eq!(refused(&calls.call(&receive_request(h, &long))), "UR-16");
    let huge = receive_request(h, &vec![b'a'; MAX_CALL]);
    assert_eq!(refused(&calls.call(&huge)), BAD_FRAMING);
}

// ---- the framing ---------------------------------------------------------------------

/// One valid request of each operation, with the offsets of its length fields.
fn samples() -> Vec<(Vec<u8>, Vec<usize>)> {
    let ur = message(8);
    vec![
        (qr_request(1, b"HELLO"), vec![7, 8, 9, 10]),
        (ur_request("bytes", 100, &ur), vec![6, 14, 15, 16, 17]),
        (new_request(), vec![]),
        (receive_request(1, b"ur:bytes/x"), vec![10, 11, 12, 13]),
        (handle_request(OP_DECODER_RESULT, 1), vec![]),
        (handle_request(OP_DECODER_DROP, 1), vec![]),
        (
            key_request("bytes", &encode_bytes(b"{}")),
            vec![6, 12, 13, 14, 15],
        ),
    ]
}

#[test]
fn ur_21_every_cut_of_a_request_is_refused() {
    for (request, _) in samples() {
        let calls = Calls::new();
        for cut in 0..request.len() {
            assert_eq!(
                refused(&calls.call(&request[..cut])),
                BAD_FRAMING,
                "op {} cut at {cut}",
                request[5]
            );
        }
    }
}

#[test]
fn ur_21_an_extra_byte_after_a_request_is_refused() {
    for (request, _) in samples() {
        let calls = Calls::new();
        for extra in [0u8, 1, 0xff] {
            let mut longer = request.clone();
            longer.push(extra);
            assert_eq!(
                refused(&calls.call(&longer)),
                BAD_FRAMING,
                "op {}",
                request[5]
            );
        }
    }
}

#[test]
fn ur_21_a_flipped_bit_in_the_magic_the_version_or_a_length_is_refused() {
    for (request, lengths) in samples() {
        let calls = Calls::new();
        let guarded: Vec<usize> = (0..5).chain(lengths).collect();
        for at in guarded {
            for bit in 0..8 {
                let mut flipped = request.clone();
                flipped[at] ^= 1 << bit;
                assert_eq!(
                    refused(&calls.call(&flipped)),
                    BAD_FRAMING,
                    "op {} byte {at} bit {bit}",
                    request[5]
                );
            }
        }
    }
}

#[test]
fn ur_21_a_flipped_bit_anywhere_gives_a_framed_reply() {
    // The operation byte may turn into another operation and a field may
    // still be valid: whatever it becomes, the reply is framed and nothing
    // panics.
    for (request, _) in samples() {
        let calls = Calls::new();
        for at in 0..request.len() {
            for bit in 0..8 {
                let mut flipped = request.clone();
                flipped[at] ^= 1 << bit;
                let _ = parse(&calls.call(&flipped));
            }
        }
    }
}

#[test]
fn ur_21_unknown_operations_and_versions_are_refused() {
    let calls = Calls::new();
    for op in [0u8, 7, 8, 0x7f, 0xff] {
        assert_eq!(refused(&calls.call(&head(op))), BAD_FRAMING, "op {op}");
    }
    let mut v = new_request();
    v[4] = 2;
    assert_eq!(refused(&calls.call(&v)), BAD_FRAMING);
    assert_eq!(refused(&calls.call(&[])), BAD_FRAMING);
    // The reply's magic is not a request.
    let mut v = new_request();
    v[..4].copy_from_slice(&REPLY_MAGIC);
    assert_eq!(refused(&calls.call(&v)), BAD_FRAMING);
}

#[test]
fn ur_21_a_length_field_that_claims_more_than_is_there_is_refused() {
    let calls = Calls::new();
    let mut v = head(OP_DECODER_RECEIVE);
    v.extend_from_slice(&1u32.to_be_bytes());
    v.extend_from_slice(&u32::MAX.to_be_bytes());
    v.extend_from_slice(b"short");
    assert_eq!(refused(&calls.call(&v)), BAD_FRAMING);
    let mut v = head(OP_QR_ENCODE);
    v.push(0);
    v.extend_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(refused(&calls.call(&v)), BAD_FRAMING);
}

#[test]
fn ur_21_no_bytes_make_the_call_panic() {
    // A small deterministic spread of garbage after a good head.
    let calls = Calls::new();
    let mut x = 0x2545_f491u32;
    for op in 0..=8u8 {
        for len in 0..40usize {
            let mut v = head(op);
            for _ in 0..len {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                v.push(x as u8);
            }
            let _ = parse(&calls.call(&v));
        }
    }
}

// ---- the size of one frame ---------------------------------------------------------

#[test]
fn qr_jni_call_a_part_of_230_bytes_fits_version_12_at_level_l() {
    // The largest fragment the apps use (`AnimatedQrView`): every part of a
    // sequence, for either type and messages up to the largest in practice,
    // draws as a code of version 12 or less in the alphanumeric mode.
    let calls = Calls::new();
    let mut psbt = b"psbt\xff".to_vec();
    psbt.extend(message(100_000));
    for (ur_type, source) in [("bytes", message(100_000)), ("crypto-psbt", psbt)] {
        for len in [5usize, 100, 229, 230, 231, 460, 5_000, 100_000] {
            let payload = source[..len].to_vec();
            let body = parse(&calls.call(&ur_request(ur_type, 230, &payload))).expect("parts");
            for part in parts(&body) {
                let code = encode_with(part.as_bytes(), Ecc::L, None, None).expect("fits");
                assert!(
                    code.version() <= 12,
                    "{ur_type} {len}: version {}",
                    code.version()
                );
                assert_eq!(code.mode(), openbmt_qr::Mode::Alphanumeric);
            }
        }
    }
}

// ---- key_text (UR-31) -----------------------------------------------------------

/// The Keystone layout of a P2WSH cosigner export with a key of the
/// fixed seed [3; 32] at m/48'/0'/0'/2' (also in openbmt-ur's tests).
fn keystone_account() -> Vec<u8> {
    let hex = "a2011a56c4fac30281d90191d9012fa602f4035821\
        02d44daf959d7b3bf9343e2aea84ea21391ccf306a2765c35859c94cecb870aaea\
        045820f4ef1818131a780976e9dc1f30a4bd39903a9b8bbffad099035c49f886e85aed\
        05d90131a20100020006d90130a301881830f500f500f502f5021a56c4fac3030408\
        1ac07436a2";
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex"))
        .collect()
}

const KEYSTONE_TEXT: &str = "[56c4fac3/48h/0h/0h/2h]xpub6Ewx2N9hNSArJyF35CUGhaZLuZxQPNmJzWVwmpoV9U7Xu5wqka93nd3zEzokew9MzkNV4u6TCVDkHHR6QHQuYEFaasKzWkrkncXHMXGNdZP";

#[test]
fn ur_31_the_key_text_of_a_finished_ur_comes_back() {
    let calls = Calls::new();
    let body = parse(&calls.call(&key_request("crypto-account", &keystone_account())))
        .expect("the key text");
    assert_eq!(String::from_utf8(body).expect("ASCII"), KEYSTONE_TEXT);
    let json = format!(
        "{{\"xfp\": \"56C4FAC3\", \"p2wsh_deriv\": \"m/48'/0'/0'/2'\", \"p2wsh\": \"{}\"}}",
        KEYSTONE_TEXT.split_once(']').expect("origin").1
    );
    let body = parse(&calls.call(&key_request("bytes", &encode_bytes(json.as_bytes()))))
        .expect("the key text");
    assert_eq!(String::from_utf8(body).expect("ASCII"), KEYSTONE_TEXT);
}

#[test]
fn ur_31_a_refusal_of_the_key_carries_the_rule_and_the_words() {
    let calls = Calls::new();
    for (ur_type, message, rule, heading) in [
        ("hdkey", keystone_account(), "UR-24", "Code type not read"),
        (
            "crypto-output",
            keystone_account(),
            "UR-24",
            "Not a key code",
        ),
        ("crypto-hdkey", vec![0xf6], "UR-23", "Key code not readable"),
        (
            "bytes",
            encode_bytes(b"not json"),
            "UR-29",
            "Key text not readable",
        ),
    ] {
        let reply = calls.call(&key_request(ur_type, &message));
        let (code, [h, what, next]) = words_of(&reply);
        assert_eq!((code.as_str(), h.as_str()), (rule, heading), "{ur_type}");
        assert!(what.ends_with('.') && next.ends_with('.'));
        let w = openbmt_ur::ur_to_key_text(ur_type, &message)
            .expect_err("a refusal")
            .words();
        assert_eq!(
            (h.as_str(), what.as_str(), next.as_str()),
            (w.heading, w.what, w.next)
        );
    }
}

#[test]
fn ur_31_the_call_gives_the_cbor_of_the_key_types() {
    let calls = Calls::new();
    let cbor = keystone_account();
    for ur_type in [
        "crypto-account",
        "crypto-hdkey",
        "hdkey",
        "account-descriptor",
        "output-descriptor",
    ] {
        let text = whole(ur_type, &cbor);
        let h = open(&calls);
        parse(&calls.call(&receive_request(h, text.as_bytes()))).expect("complete");
        let body = parse(&calls.call(&handle_request(OP_DECODER_RESULT, h))).expect("result");
        assert_eq!(body[0] as usize, ur_type.len());
        assert_eq!(&body[1..1 + ur_type.len()], ur_type.as_bytes());
        assert_eq!(&body[5 + ur_type.len()..], cbor.as_slice(), "{ur_type}");
        calls.call(&handle_request(OP_DECODER_DROP, h));
    }
    // A sequence of several parts through a decoder and into key_text.
    let mut e = Encoder::new("crypto-account", &cbor, 30, 10).expect("encoder");
    let h = open(&calls);
    let mut complete = false;
    for _ in 0..e.seq_len() {
        let p = parse(&calls.call(&receive_request(h, e.next_part().as_bytes()))).expect("part");
        complete = p[0] == 1;
    }
    assert!(complete);
    let body = parse(&calls.call(&handle_request(OP_DECODER_RESULT, h))).expect("result");
    let n = body[0] as usize;
    let got = &body[5 + n..];
    let key = parse(&calls.call(&key_request("crypto-account", got))).expect("key");
    assert_eq!(String::from_utf8(key).expect("ASCII"), KEYSTONE_TEXT);
}

#[test]
fn ur_31_a_request_that_is_not_framed_or_has_a_bad_type_is_refused() {
    let calls = Calls::new();
    let good = key_request("crypto-account", &keystone_account());
    for cut in 0..good.len() {
        assert_eq!(refused(&calls.call(&good[..cut])), BAD_FRAMING, "cut {cut}");
    }
    let mut extra = good.clone();
    extra.push(0);
    assert_eq!(refused(&calls.call(&extra)), BAD_FRAMING);
    assert_eq!(
        refused(&calls.call(&key_request("Crypto-Account", &[1]))),
        "UR-01"
    );
    assert_eq!(refused(&calls.call(&key_request("", &[1]))), "UR-01");
}

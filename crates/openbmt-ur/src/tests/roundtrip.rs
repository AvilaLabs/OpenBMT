//! Round trips, loss, order, repeats, case, and a decoder that does not
//! panic.

use super::make_message;
use crate::xoshiro::Xoshiro256;
use crate::{cbor, psbt_to_ur_parts, ur_to_psbt, Decoder, Encoder};

fn rand_below(rng: &mut Xoshiro256, n: usize) -> usize {
    rng.next_int(0, n as u64 - 1) as usize
}

#[test]
fn ur_19_a_single_part_ur_decodes_at_once() {
    let payload = cbor::encode_bytes(b"short");
    let mut e = Encoder::new("bytes", &payload, 100, 10).expect("ok");
    assert!(e.is_single_part());
    assert_eq!(e.seq_len(), 1);
    let text = e.next_part();
    assert_eq!(text, e.next_part(), "a single part repeats");
    assert!(text.starts_with("ur:bytes/") && text.matches('/').count() == 1);
    let mut d = Decoder::new();
    let p = d.receive(&text).expect("good");
    assert!(p.complete && p.estimated_fraction == 1.0);
    assert_eq!((p.received_parts, p.expected_parts), (1, 1));
    assert_eq!(d.result(), Some(("bytes".to_string(), payload)));
}

#[test]
fn ur_19_upper_case_and_lower_case_both_decode() {
    for len in [20usize, 500] {
        let payload = cbor::encode_bytes(&make_message(len, "case"));
        let mut e = Encoder::new("bytes", &payload, 40, 10).expect("ok");
        let mut d = Decoder::new();
        for _ in 0..200 {
            let upper = e.next_part_upper();
            assert_eq!(upper, upper.to_ascii_uppercase());
            assert!(upper.starts_with("UR:BYTES/"));
            if d.receive(&upper).expect("good").complete {
                break;
            }
        }
        assert_eq!(d.result(), Some(("bytes".to_string(), payload.clone())));
        // And the lower-case form of a mixed-case text.
        let mut e = Encoder::new("bytes", &payload, 40, 10).expect("ok");
        let mut d = Decoder::new();
        for i in 0..200 {
            let t = e.next_part();
            let t = if i % 2 == 0 {
                t
            } else {
                t.to_ascii_uppercase()
            };
            if d.receive(&t).expect("good").complete {
                break;
            }
        }
        assert!(d.is_complete());
    }
}

#[test]
fn ur_19_the_first_seq_len_parts_alone_complete_the_message() {
    for len in [11usize, 100, 1000, 5000] {
        let payload = cbor::encode_bytes(&make_message(len, "plain"));
        let mut e = Encoder::new("bytes", &payload, 77, 10).expect("ok");
        let mut d = Decoder::new();
        for i in 0..e.seq_len() {
            let p = d.receive(&e.next_part()).expect("good");
            assert_eq!(p.complete, i + 1 == e.seq_len(), "len {len} part {i}");
        }
        assert_eq!(d.result().map(|r| r.1), Some(payload));
    }
}

#[test]
fn ur_19_thirty_percent_lost_shuffled_and_repeated_still_decodes() {
    for (len, seed) in [(300usize, "a"), (1500, "b"), (6000, "c"), (20_000, "d")] {
        let payload = cbor::encode_bytes(&make_message(len, seed));
        let mut e = Encoder::new("crypto-psbt", &payload, 100, 10).expect("ok");
        let n = e.seq_len();
        let mut rng = Xoshiro256::from_bytes(seed.as_bytes());
        // Three times the parts the message needs, 30% dropped.
        let mut kept: Vec<String> = Vec::new();
        for _ in 0..3 * n {
            let t = e.next_part();
            if rand_below(&mut rng, 100) >= 30 {
                kept.push(t);
            }
        }
        // Repeats: a third of the kept parts again.
        let extra: Vec<String> = kept
            .iter()
            .filter(|_| rand_below(&mut rng, 3) == 0)
            .cloned()
            .collect();
        kept.extend(extra);
        // A shuffle.
        for i in (1..kept.len()).rev() {
            kept.swap(i, rand_below(&mut rng, i + 1));
        }
        let mut d = Decoder::new();
        let mut finished = false;
        for t in &kept {
            let p = d.receive(t).expect("good part");
            assert!(p.estimated_fraction <= 1.0);
            assert_eq!(p.expected_parts, n);
            if p.complete {
                finished = true;
                break;
            }
        }
        assert!(
            finished,
            "len {len}: {} parts kept of {}",
            kept.len(),
            3 * n
        );
        assert_eq!(d.result(), Some(("crypto-psbt".to_string(), payload)));
    }
}

#[test]
fn ur_19_progress_grows_and_is_not_complete_before_the_end() {
    let payload = cbor::encode_bytes(&make_message(900, "prog"));
    let mut e = Encoder::new("bytes", &payload, 50, 10).expect("ok");
    let mut d = Decoder::new();
    let mut last = 0.0;
    let mut parts = 0;
    loop {
        let p = d.receive(&e.next_part()).expect("good");
        parts += 1;
        assert_eq!(p.received_parts, parts);
        assert!(p.estimated_fraction >= last);
        last = p.estimated_fraction;
        if p.complete {
            assert_eq!(p.estimated_fraction, 1.0);
            break;
        }
        assert!(p.estimated_fraction < 1.0);
    }
    // A repeat of a part counts once.
    let mut d = Decoder::new();
    let t = Encoder::new("bytes", &payload, 50, 10)
        .expect("ok")
        .next_part();
    d.receive(&t).expect("good");
    assert_eq!(d.receive(&t).expect("good").received_parts, 1);
}

#[test]
fn ur_19_a_psbt_goes_through_crypto_psbt() {
    let mut psbt = b"psbt\xff".to_vec();
    psbt.extend(make_message(3000, "psbt"));
    let mut e = psbt_to_ur_parts(&psbt, 150).expect("ok");
    assert!(!e.is_single_part());
    let mut d = Decoder::new();
    while !d.is_complete() {
        d.receive(&e.next_part_upper()).expect("good");
    }
    let (ty, cbor) = d.result().expect("done");
    assert_eq!(ur_to_psbt(&ty, &cbor), Ok(psbt));
}

#[test]
fn ur_18_a_refused_part_leaves_the_decoder_as_it_was() {
    let payload = cbor::encode_bytes(&make_message(400, "keep"));
    let mut e = Encoder::new("bytes", &payload, 40, 10).expect("ok");
    let parts: Vec<String> = (0..e.seq_len()).map(|_| e.next_part()).collect();
    let mut d = Decoder::new();
    let first = d.receive(&parts[0]).expect("good");
    let junk = [
        "",
        "ur:bytes/2-11/zzzz",
        "ur:other/2-11/aeadaolazmjendeoti",
        &parts[1].replace("bytes", "psbt"),
        &parts[1][..parts[1].len() - 2],
    ];
    for j in junk {
        assert!(d.receive(j).is_err(), "{j}");
        assert_eq!(d.held_mixed(), 0);
    }
    // The first part again is a repeat, not a change.
    assert_eq!(d.receive(&parts[0]), Ok(first));
    for p in &parts[1..] {
        d.receive(p).expect("good");
    }
    assert_eq!(d.result().map(|r| r.1), Some(payload));
}

#[test]
fn ur_17_a_decoder_never_panics_on_any_text() {
    let payload = cbor::encode_bytes(&make_message(300, "fuzz"));
    let mut e = Encoder::new("bytes", &payload, 40, 10).expect("ok");
    let valid: Vec<String> = (0..12).map(|_| e.next_part()).collect();
    let mut rng = Xoshiro256::from_bytes(b"fuzz");
    let alphabet: Vec<char> = "ur:bytes/-0123456789abcdefghijklmnopqrstuvwxyzABCZ é日\u{1F600}"
        .chars()
        .collect();
    let mut d = Decoder::new();
    for text in &valid {
        // Every prefix, and a change of each character in turn.
        let chars: Vec<char> = text.chars().collect();
        for cut in 0..chars.len() {
            let s: String = chars.iter().take(cut).collect();
            let _ = d.receive(&s);
        }
        for at in 0..chars.len() {
            let mut c = chars.clone();
            if let Some(x) = c.get_mut(at) {
                *x = alphabet[rand_below(&mut rng, alphabet.len())];
            }
            let s: String = c.into_iter().collect();
            let _ = d.receive(&s);
        }
        // The decoder still works after all of that.
        let _ = d.receive(text);
    }
    // Garbage of every shape, including long and non-ASCII.
    for n in 0..3000 {
        let len = rand_below(&mut rng, 80);
        let s: String = (0..len)
            .map(|_| alphabet[rand_below(&mut rng, alphabet.len())])
            .collect();
        let _ = d.receive(&s);
        let _ = d.receive(&format!("ur:bytes/{s}"));
        let _ = d.receive(&format!("ur:bytes/{}-{}/{s}", n % 7, n % 13));
    }
    // And the decoders that start clean on the valid parts still finish.
    let mut d = Decoder::new();
    for t in &valid {
        let _ = d.receive(t);
    }
    assert!(d.is_complete());
}

//! BQ-01 to BQ-10, BQ-13 and BQ-16: the text of parts, sets of parts and
//! the writer.

use super::{hex_upper, joiner, sample};
use crate::{
    base32, encode, BbqrError, Joiner, Kind, ALL_REFUSALS, MAX_DECODED, MAX_PARTS, MAX_PART_CHARS,
};

fn read_all(parts: &[String]) -> (Kind, Vec<u8>) {
    let mut j = joiner();
    for p in parts {
        j.add(p).expect("part");
    }
    let d = j.finish().expect("finish");
    (d.kind, d.bytes)
}

/// Header and data of a hand-made part.
fn part(enc: char, kind: char, total: &str, index: &str, data: &str) -> String {
    format!("B${enc}{kind}{total}{index}{data}")
}

#[test]
fn bq_01_the_header_has_the_shape_of_the_format() {
    let parts = encode(Kind::Psbt, b"psbt\xff", 100).expect("encode");
    assert_eq!(parts, vec!["B$2P0100OBZWE5H7".to_string()]);
    assert_eq!(read_all(&parts), (Kind::Psbt, b"psbt\xff".to_vec()));
}

#[test]
fn bq_01_refuses_text_that_is_not_a_part() {
    for bad in [
        "",
        "B$",
        "B$2P010",
        "X$2P0100OBZWE5H7",
        "B-2P0100OBZWE5H7",
        "B$2P01\u{e9}0OBZWE5H7",
        "B$2P0!00OBZWE5H7",
        "B$2P0100OBZWE5H7 ",
    ] {
        assert!(
            matches!(
                joiner().add(bad),
                Err(BbqrError::Header(_) | BbqrError::Count | BbqrError::Base32(_))
            ),
            "{bad:?}"
        );
    }
    let long = format!("B$2P0100{}", "A".repeat(MAX_PART_CHARS));
    assert!(matches!(joiner().add(&long), Err(BbqrError::Size(_))));
}

#[test]
fn bq_01_reads_lower_case_and_writes_upper_case() {
    let parts = encode(Kind::Psbt, &sample(300), 120).expect("encode");
    assert!(parts.iter().all(|p| p
        .chars()
        .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase() || c == '$')));
    let lower: Vec<String> = parts.iter().map(|p| p.to_ascii_lowercase()).collect();
    assert_eq!(read_all(&lower), (Kind::Psbt, sample(300)));
}

#[test]
fn bq_02_reads_hex_base32_and_the_writer_writes_base32() {
    let data = sample(40);
    let hex = part('H', 'P', "01", "00", &hex_upper(&data));
    assert_eq!(read_all(&[hex]), (Kind::Psbt, data.clone()));
    let b32 = part('2', 'T', "01", "00", &base32::encode(&data));
    assert_eq!(read_all(&[b32]), (Kind::Transaction, data.clone()));
    let written = encode(Kind::Psbt, &data, 4296).expect("encode");
    assert!(written.iter().all(|p| p.chars().nth(2) == Some('2')));
}

#[test]
fn bq_02_refuses_another_encoding() {
    for e in ['3', 'A', '8', 'Q', '1'] {
        let p = part(e, 'P', "01", "00", "AAAA");
        assert_eq!(joiner().add(&p), Err(BbqrError::Encoding), "{e}");
    }
}

#[test]
fn bq_03_a_reader_takes_only_its_kinds() {
    let p = encode(Kind::Json, b"{}", 100).expect("encode");
    assert_eq!(joiner().add(&p[0]), Err(BbqrError::Kind));
    let mut any = Joiner::new(&[Kind::Json]);
    any.add(&p[0]).expect("json");
    assert_eq!(any.finish().expect("json").kind, Kind::Json);
    let unknown = part('2', 'Z', "01", "00", "AAAA");
    assert_eq!(joiner().add(&unknown), Err(BbqrError::Kind));
    for kind in [
        Kind::Psbt,
        Kind::Transaction,
        Kind::Json,
        Kind::Cbor,
        Kind::Text,
        Kind::Binary,
        Kind::Executable,
    ] {
        assert_eq!(Kind::from_code(kind.code()), Some(kind));
    }
}

#[test]
fn bq_04_refuses_a_count_or_number_out_of_range() {
    for (total, index) in [("00", "00"), ("01", "01"), ("03", "03"), ("03", "ZZ")] {
        let p = part('2', 'P', total, index, "AAAAAAAA");
        assert_eq!(joiner().add(&p), Err(BbqrError::Count), "{total} {index}");
    }
    // The most parts is ZZ, 1295.
    let p = part('2', 'P', "ZZ", "00", "AAAAAAAA");
    assert!(joiner().add(&p).is_ok());
    assert_eq!(MAX_PARTS, 36 * 36 - 1);
}

#[test]
fn bq_05_sizes_are_bounded() {
    assert!(matches!(
        encode(Kind::Psbt, &[], 100),
        Err(BbqrError::Size(_))
    ));
    assert!(matches!(
        encode(Kind::Psbt, &[1], 15),
        Err(BbqrError::Size(_))
    ));
    assert!(matches!(
        encode(Kind::Psbt, &[1], MAX_PART_CHARS + 1),
        Err(BbqrError::Size(_))
    ));
    assert!(matches!(
        encode(Kind::Psbt, &vec![0u8; MAX_DECODED + 1], MAX_PART_CHARS),
        Err(BbqrError::Size(_))
    ));
    // A part with no data.
    assert!(matches!(joiner().add("B$2P0100"), Err(BbqrError::Size(_))));
    // The most parts at the shortest length is more than 1295.
    assert!(matches!(
        encode(Kind::Psbt, &vec![7u8; 8000], 16),
        Err(BbqrError::Count)
    ));
}

#[test]
fn bq_06_a_part_that_disagrees_is_refused_and_changes_nothing() {
    let parts = encode(Kind::Psbt, &sample(200), 100).expect("encode");
    assert!(parts.len() >= 3);
    let mut j = joiner();
    j.add(&parts[0]).expect("first");
    let other_kind = parts[1].replacen("B$2P", "B$2T", 1);
    assert!(matches!(j.add(&other_kind), Err(BbqrError::Disagrees(_))));
    let other_total = {
        let mut p = parts[1].clone();
        p.replace_range(4..6, "0A");
        p
    };
    assert!(matches!(j.add(&other_total), Err(BbqrError::Disagrees(_))));
    let hex = part('H', 'P', &parts[1][4..6], "01", "AA");
    assert!(matches!(j.add(&hex), Err(BbqrError::Disagrees(_))));
    // The same number with other data.
    let mut altered = parts[0].clone();
    altered.replace_range(8..16, "AAAAAAAA");
    assert!(matches!(j.add(&altered), Err(BbqrError::Disagrees(_))));
    // None of that changed what is held: the set still completes.
    assert_eq!(j.progress().map(|p| p.have), Some(1));
    for p in &parts[1..] {
        j.add(p).expect("rest");
    }
    assert_eq!(j.finish().expect("done").bytes, sample(200));
}

#[test]
fn bq_06_a_repeat_is_not_counted_twice() {
    let parts = encode(Kind::Psbt, &sample(200), 100).expect("encode");
    let mut j = joiner();
    for _ in 0..3 {
        let p = j.add(&parts[1]).expect("repeat");
        assert_eq!(p.have, 1);
    }
    assert!(!j.progress().expect("progress").complete());
}

#[test]
fn bq_07_every_part_but_the_last_has_one_length() {
    let mut j = joiner();
    j.add(&part('2', 'P', "03", "00", "AAAAAAAA")).expect("a");
    assert!(matches!(
        j.add(&part('2', 'P', "03", "01", "AAAAAAAAAAAAAAAA")),
        Err(BbqrError::Shape(_))
    ));
    // Not whole groups of eight in a part that is not the last.
    assert!(matches!(
        joiner().add(&part('2', 'P', "02", "00", "AAAAAAAAAA")),
        Err(BbqrError::Shape(_))
    ));
    // A last part longer than the others.
    let mut j = joiner();
    j.add(&part('2', 'P', "02", "00", "AAAAAAAA")).expect("a");
    assert!(matches!(
        j.add(&part('2', 'P', "02", "01", "AAAAAAAAAA")),
        Err(BbqrError::Shape(_))
    ));
    // The last first, and then a part longer than it.
    let mut j = joiner();
    j.add(&part('2', 'P', "02", "01", "AAAAAAAAAA"))
        .expect("last");
    assert!(matches!(
        j.add(&part('2', 'P', "02", "00", "AAAAAAAA")),
        Err(BbqrError::Shape(_))
    ));
}

#[test]
fn bq_08_hex_is_whole_bytes_of_hex_digits() {
    for bad in ["ABC", "GG", "AB CD", "0x", "AB=="] {
        assert_eq!(
            joiner().add(&part('H', 'P', "01", "00", bad)),
            Err(BbqrError::Hex),
            "{bad}"
        );
    }
    let ok = part('H', 'T', "02", "00", "AABB");
    let ok2 = part('H', 'T', "02", "01", "CC");
    assert_eq!(
        read_all(&[ok2, ok]),
        (Kind::Transaction, vec![0xaa, 0xbb, 0xcc])
    );
}

#[test]
fn bq_09_base32_is_strict() {
    // One byte is two characters; the two spare bits must be zero.
    assert!(joiner().add(&part('2', 'P', "01", "00", "AA")).is_ok());
    assert!(matches!(
        joiner().add(&part('2', 'P', "01", "00", "AB")),
        Err(BbqrError::Base32(_))
    ));
    for bad in [
        "AAA",
        "AAAAAA",
        "A",
        "AAAAAAAAA",
        "AA==",
        "AA=AAAAA",
        "A1AAAAAA",
        "AA AAAA",
    ] {
        assert!(
            matches!(
                joiner().add(&part('2', 'P', "01", "00", bad)),
                Err(BbqrError::Base32(_))
            ),
            "{bad}"
        );
    }
    // Every number of bytes from 1 to 12 reads back.
    for n in 1..=12usize {
        let data = sample(n);
        let p = part('2', 'P', "01", "00", &base32::encode(&data));
        assert_eq!(read_all(&[p]).1, data, "{n}");
    }
}

#[test]
fn bq_10_the_file_comes_only_when_every_part_is_in() {
    let data = sample(500);
    let parts = encode(Kind::Psbt, &data, 120).expect("encode");
    let n = parts.len();
    assert!(n >= 4);
    let mut j = joiner();
    assert_eq!(j.finish(), Err(BbqrError::Missing { have: 0, total: 1 }));
    // Any order: last first, then from the back.
    for (k, p) in parts.iter().rev().enumerate() {
        if k + 1 < n {
            let progress = j.add(p).expect("part");
            assert_eq!(progress.have, k + 1);
            assert_eq!(
                j.finish(),
                Err(BbqrError::Missing {
                    have: k + 1,
                    total: n
                })
            );
        } else {
            j.add(p).expect("part");
        }
    }
    assert_eq!(j.finish().expect("done").bytes, data);
    // Asking changed nothing.
    assert_eq!(j.finish().expect("again").bytes, data);
}

#[test]
fn bq_10_the_parts_read_in_any_order_with_repeats() {
    let data = sample(3000);
    let parts = encode(Kind::Transaction, &data, 300).expect("encode");
    let n = parts.len();
    // Interleave from both ends, with each repeated once.
    let mut order: Vec<usize> = Vec::new();
    for i in 0..n {
        order.push(if i % 2 == 0 { i / 2 } else { n - 1 - i / 2 });
    }
    let mut j = joiner();
    for i in order.iter().chain(order.iter()) {
        j.add(&parts[*i]).expect("part");
    }
    let done = j.finish().expect("done");
    assert_eq!((done.kind, done.bytes), (Kind::Transaction, data));
}

#[test]
fn bq_13_the_writer_cuts_into_equal_parts_and_a_shorter_last() {
    for (len, max) in [
        (1, 16),
        (5, 100),
        (6, 16),
        (100, 40),
        (999, 200),
        (5000, 4296),
    ] {
        let data = sample(len);
        let parts = encode(Kind::Psbt, &data, max).expect("encode");
        assert!(parts.iter().all(|p| p.len() <= max), "{len} {max}");
        let (last, rest) = parts.split_last().expect("one part");
        for p in rest {
            assert_eq!(p.len(), rest[0].len());
            assert_eq!((p.len() - 8) % 8, 0);
        }
        if let Some(first) = rest.first() {
            assert!(last.len() <= first.len());
        }
        // The fewest parts: one fewer would not fit.
        let text = base32::encode(&data).len();
        let room = (max - 8) / 8 * 8;
        assert_eq!(parts.len(), text.div_ceil(room), "{len} {max}");
        assert_eq!(read_all(&parts), (Kind::Psbt, data));
    }
    assert_eq!(
        encode(Kind::Text, b"hello", 100).expect("text")[0],
        "B$2U0100NBSWY3DP"
    );
}

#[test]
fn bq_16_every_refusal_has_words_that_pass_the_screen_checks() {
    use openbmt_core::say::{check, check_distinct, Tone, Who, Words};
    let mut all = Vec::new();
    for r in ALL_REFUSALS {
        let w = r.words();
        let words = Words::new(Tone::Stop, w.heading, w.what, Who::Owner, w.next).code(r.rule());
        check(&words).unwrap_or_else(|e| panic!("{r:?}: {e}"));
        all.push((format!("{r:?}"), words));
        for text in [w.heading, w.what, w.next] {
            assert!(!text.contains("BQ-"), "{r:?}");
        }
        let shown = r.to_string();
        assert!(shown.starts_with(r.rule()), "{shown}");
        assert!(shown.len() > r.rule().len() + 2, "{shown}");
    }
    check_distinct(&all).expect("distinct");
}

#[test]
fn bq_16_the_list_of_refusals_is_every_variant() {
    let index = |r: &BbqrError| -> usize {
        match r {
            BbqrError::Header(_) => 0,
            BbqrError::Encoding => 1,
            BbqrError::Kind => 2,
            BbqrError::Count => 3,
            BbqrError::Size(_) => 4,
            BbqrError::Disagrees(_) => 5,
            BbqrError::Shape(_) => 6,
            BbqrError::Hex => 7,
            BbqrError::Base32(_) => 8,
            BbqrError::Missing { .. } => 9,
            BbqrError::Inflate(_) => 10,
            BbqrError::InflateTooBig => 11,
        }
    };
    for (i, r) in ALL_REFUSALS.iter().enumerate() {
        assert_eq!(index(r), i);
    }
}

#[test]
fn bq_11_a_zlib_set_joins_and_unpacks() {
    use crate::inflate::inflate;
    // A set in the Z encoding: the stored vector, in base32, in two parts.
    let raw: Vec<u8> = {
        let mut v = vec![0x01, 0x14, 0x00, 0xeb, 0xff];
        v.extend(0u8..20);
        v
    };
    assert_eq!(inflate(&raw, MAX_DECODED).expect("stored").len(), 20);
    let text = base32::encode(&raw);
    let (a, b) = text.split_at(24);
    let p0 = part('Z', 'P', "02", "00", a);
    let p1 = part('Z', 'P', "02", "01", b);
    assert_eq!(
        read_all(&[p1, p0]),
        (Kind::Psbt, (0u8..20).collect::<Vec<_>>())
    );
}

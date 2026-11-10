//! The contents of the Records and Records received kinds (LK-54, LK-55).

use openbmt_core::records::GENESIS;
use openbmt_core::wire::DecodeError;
use openbmt_link::seal::MAX_CONTENT_LEN;

use super::kit::*;
use crate::{
    decode_received, decode_records, head_at, received_content, records_content, WireError,
    RECEIVED_LEN,
};

#[test]
fn lk_54_records_go_in_and_come_out_whole_and_in_order() {
    let rs = records(5, 40);
    let (content, count) = records_content(&rs, MAX_CONTENT_LEN);
    assert_eq!(count, 5);
    assert_eq!(decode_records(&content).unwrap(), rs);
    // Each is its length, then the record's own bytes.
    let first = rs[0].to_bytes();
    assert_eq!(&content[..4], &(first.len() as u32).to_le_bytes());
    assert_eq!(&content[4..4 + first.len()], first.as_slice());
}

#[test]
fn lk_54_only_whole_records_that_fit_are_put_in() {
    let rs = records(10, 100);
    let one = rs[0].to_bytes().len() + 4;
    let (content, count) = records_content(&rs, one * 3 + one / 2);
    assert_eq!(count, 3);
    assert_eq!(content.len(), one * 3);
    assert_eq!(decode_records(&content).unwrap(), rs[..3].to_vec());
    // Never more than one message holds, whatever the caller asks for.
    let big = records(200, 1_000);
    let (content, count) = records_content(&big, usize::MAX);
    assert!(content.len() <= MAX_CONTENT_LEN);
    assert!(count > 50 && count < 200);
    // A record that alone is too long gives nothing.
    let (content, count) = records_content(&rs, one - 1);
    assert!(content.is_empty());
    assert_eq!(count, 0);
}

#[test]
fn lk_54_a_content_that_is_not_whole_records_is_refused_whole() {
    let rs = records(3, 30);
    let (content, _) = records_content(&rs, MAX_CONTENT_LEN);
    assert_eq!(decode_records(&[]).unwrap_err(), WireError::Empty);
    // Cut inside the last record, inside a length, after a length.
    for cut in [1, 5, content.len() - 1] {
        assert_eq!(
            decode_records(&content[..content.len() - cut]).unwrap_err(),
            WireError::Cut,
            "cut {cut}"
        );
    }
    assert_eq!(decode_records(&content[..2]).unwrap_err(), WireError::Cut);
    // Anything after the last record.
    let mut more = content.clone();
    more.push(0);
    assert_eq!(decode_records(&more).unwrap_err(), WireError::Cut);
    more.extend_from_slice(&[0, 0, 0]);
    assert!(matches!(
        decode_records(&more).unwrap_err(),
        WireError::Record(_)
    ));
    // A record that does not parse (its magic), and a zero length.
    let mut bad = content.clone();
    bad[4] ^= 0xff;
    assert_eq!(
        decode_records(&bad).unwrap_err(),
        WireError::Record(DecodeError::BadMagic)
    );
    assert!(matches!(
        decode_records(&[0, 0, 0, 0]).unwrap_err(),
        WireError::Record(_)
    ));
    // A length that claims more than the content holds.
    assert_eq!(
        decode_records(&[255, 255, 255, 255, 1, 2]).unwrap_err(),
        WireError::Cut
    );
}

#[test]
fn lk_55_a_receipt_is_a_count_and_a_head() {
    let head = [7u8; 32];
    let content = received_content(300, &head);
    assert_eq!(content.len(), RECEIVED_LEN);
    assert_eq!(&content[..8], &300u64.to_le_bytes());
    assert_eq!(decode_received(&content).unwrap(), (300, head));
    for n in [0, 39, 41] {
        assert_eq!(
            decode_received(&vec![0u8; n]).unwrap_err(),
            WireError::ReceivedLength(n)
        );
    }
}

#[test]
fn lk_55_the_head_after_n_records_is_the_hash_of_the_last_of_them() {
    let rs = records(4, 10);
    assert_eq!(head_at(&rs, 0), Some(GENESIS));
    assert_eq!(head_at(&rs, 1), Some(rs[0].hash()));
    assert_eq!(head_at(&rs, 4), Some(rs[3].hash()));
    assert_eq!(head_at(&rs, 5), None);
    assert_eq!(head_at(&[], 0), Some(GENESIS));
    assert_eq!(head_at(&rs, u64::MAX), None);
}

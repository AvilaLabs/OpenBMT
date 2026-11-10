//! LK-30: the frames.

use openbmt_core::identity::Sig;
use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::proto::{
    parse_header, Direction, ProtoError, Refusal, Request, Response, FETCH_BATCH, FRAME_HEADER_LEN,
    MAX_REQUEST_BODY, MAX_RESPONSE_BODY,
};

fn requests() -> Vec<Request> {
    let reader = counter().device_id();
    let sig = Sig([7; 64]);
    vec![
        Request::Hello { nonce: [3; 32] },
        Request::Challenge,
        Request::MakeMailbox {
            reader,
            challenge: [4; 32],
            sig,
        },
        Request::Put {
            sealed: vec![1, 2, 3],
        },
        Request::Fetch {
            reader,
            challenge: [5; 32],
            sig,
            cursor: 77,
        },
        Request::Ack {
            reader,
            challenge: [6; 32],
            sig,
            up_to: 88,
        },
    ]
}

fn responses() -> Vec<Response> {
    let mut v = vec![
        Response::Hello {
            relay: id(99).device_id(),
            sig: Sig([8; 64]),
        },
        Response::Challenge { challenge: [9; 32] },
        Response::Made,
        Response::Stored { id: 12 },
        Response::Messages {
            items: vec![(3, vec![1; 10]), (5, vec![]), (9, vec![2; 300])],
            more: true,
        },
        Response::Messages {
            items: vec![],
            more: false,
        },
        Response::Acked { dropped: 4 },
    ];
    v.extend(Refusal::ALL.map(Response::Refused));
    v
}

#[test]
fn lk_30_every_frame_round_trips_and_decodes_strictly() {
    for r in requests() {
        let bytes = r.encode().unwrap();
        assert_eq!(Request::decode(&bytes).unwrap(), r);
        assert_eq!(&bytes[..4], b"OBMQ");
        assert_eq!(bytes[4], 1);
        // Trailing byte, and every truncation, and a changed length.
        let mut long = bytes.clone();
        long.push(0);
        assert!(Request::decode(&long).is_err());
        for cut in 0..bytes.len() {
            assert!(Request::decode(&bytes[..cut]).is_err(), "{r:?} cut {cut}");
        }
    }
    for r in responses() {
        let bytes = r.encode().unwrap();
        assert_eq!(Response::decode(&bytes).unwrap(), r);
        let mut long = bytes.clone();
        long.push(0);
        assert!(Response::decode(&long).is_err());
        for cut in 0..bytes.len() {
            assert!(Response::decode(&bytes[..cut]).is_err(), "{r:?} cut {cut}");
        }
    }
    // A request is not a response and the other way round.
    let q = Request::Challenge.encode().unwrap();
    assert!(Response::decode(&q).is_err());
    let a = Response::Made.encode().unwrap();
    assert!(Request::decode(&a).is_err());
}

#[test]
fn lk_30_a_frame_has_a_magic_a_version_a_type_and_a_length() {
    let bytes = Request::Hello { nonce: [3; 32] }.encode().unwrap();
    assert_eq!(bytes.len(), FRAME_HEADER_LEN + 32);
    assert_eq!(&bytes[..4], b"OBMQ");
    assert_eq!(bytes[5], 1);
    assert_eq!(u32::from_le_bytes(bytes[6..10].try_into().unwrap()), 32);
    let mut bad = bytes.clone();
    bad[0] = b'X';
    assert_eq!(
        Request::decode(&bad).unwrap_err(),
        ProtoError::Decode(DecodeError::BadMagic)
    );
    let mut bad = bytes.clone();
    bad[4] = 2;
    assert_eq!(
        Request::decode(&bad).unwrap_err(),
        ProtoError::Decode(DecodeError::UnsupportedVersion(2))
    );
    let mut bad = bytes;
    bad[5] = 0x55;
    assert_eq!(
        Request::decode(&bad).unwrap_err(),
        ProtoError::UnknownType(0x55)
    );
}

#[test]
fn lk_30_the_length_is_checked_against_the_limit_before_anything_is_read() {
    let header = |len: u32| -> [u8; FRAME_HEADER_LEN] {
        let mut h = [0u8; FRAME_HEADER_LEN];
        h[..4].copy_from_slice(b"OBMQ");
        h[4] = 1;
        h[5] = 4;
        h[6..].copy_from_slice(&len.to_le_bytes());
        h
    };
    let max = MAX_REQUEST_BODY as u32;
    assert!(parse_header(&header(max), Direction::ToRelay).is_ok());
    assert_eq!(
        parse_header(&header(max + 1), Direction::ToRelay).unwrap_err(),
        ProtoError::TooLarge
    );
    assert!(parse_header(&header(MAX_RESPONSE_BODY as u32), Direction::ToDevice).is_ok());
    assert_eq!(
        parse_header(&header(u32::MAX), Direction::ToDevice).unwrap_err(),
        ProtoError::TooLarge
    );
    // Encoding refuses too.
    let big = Request::Put {
        sealed: vec![0; MAX_REQUEST_BODY + 1],
    };
    assert_eq!(big.encode().unwrap_err(), ProtoError::TooLarge);
}

#[test]
fn lk_30_an_answer_is_canonical() {
    // A count above the batch size, ids that do not rise, a status that is
    // not 0 on an answer, an unknown refusal, and a flag that is not 0 or 1.
    let frame = |ty: u8, body: &[u8]| {
        let mut f = Vec::from(*b"OBMQ");
        f.extend_from_slice(&[1, ty]);
        f.extend_from_slice(&(body.len() as u32).to_le_bytes());
        f.extend_from_slice(body);
        f
    };
    let mut body = vec![0u8, 0];
    body.extend_from_slice(&((FETCH_BATCH as u32) + 1).to_le_bytes());
    assert!(Response::decode(&frame(0x85, &body)).is_err());
    let one = |id: u64| {
        let mut v = id.to_le_bytes().to_vec();
        v.extend_from_slice(&0u32.to_le_bytes());
        v
    };
    let mut body = vec![0u8, 0];
    body.extend_from_slice(&2u32.to_le_bytes());
    body.extend_from_slice(&one(5));
    body.extend_from_slice(&one(5));
    assert!(Response::decode(&frame(0x85, &body)).is_err());
    assert!(Response::decode(&frame(0x83, &[1])).is_err());
    assert!(Response::decode(&frame(0xff, &[0])).is_err());
    assert!(Response::decode(&frame(0xff, &[200])).is_err());
    assert!(Response::decode(&frame(0xff, &[1, 1])).is_err());
    let mut body = vec![0u8, 2];
    body.extend_from_slice(&0u32.to_le_bytes());
    assert!(Response::decode(&frame(0x85, &body)).is_err());
    // A reader key that is not on the curve.
    let mut body = vec![0u8; 32 + 32 + 64];
    body[0] = 0;
    assert!(Request::decode(&frame(3, &body)).is_err());
}

#[test]
fn lk_20_every_refusal_names_a_rule_and_has_its_own_byte() {
    let mut bytes = std::collections::BTreeSet::new();
    for r in Refusal::ALL {
        assert!(bytes.insert(r.to_byte()));
        assert_ne!(r.to_byte(), 0);
        assert_eq!(Refusal::from_byte(r.to_byte()), Some(r));
        assert!(r.to_string().starts_with(r.rule()));
        assert!(r.rule().starts_with("LK-"));
    }
    assert_eq!(Refusal::from_byte(0), None);
    assert_eq!(Refusal::from_byte(250), None);
}

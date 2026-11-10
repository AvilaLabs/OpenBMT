//! NR-92: the host name of a Tor v3 address and the SHA3-256 under its
//! checksum.

use crate::onion::{onion_name, parse_onion_name};
use crate::sha3::sha3_256;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

#[test]
fn nr_92_sha3_256_matches_the_vectors_of_fips_202() {
    let cases: [(&[u8], &str); 3] = [
        (
            b"",
            "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a",
        ),
        (
            b"abc",
            "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532",
        ),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "41c0dba2a9d6240849100376a8235e2c82e1b9998a999e21db32dd97496d3376",
        ),
    ];
    for (input, want) in cases {
        assert_eq!(hex(&sha3_256(input)), want, "{input:?}");
    }
}

#[test]
fn nr_92_sha3_256_is_right_around_the_block_size_and_for_a_million_a() {
    // The rate is 136 bytes: the padding falls in the same block, or in an
    // extra one.
    let cases = [
        (
            135,
            "8094bb53c44cfb1e67b7c30447f9a1c33696d2463ecc1d9c92538913392843c9",
        ),
        (
            136,
            "3fc5559f14db8e453a0a3091edbd2bc25e11528d81c66fa570a4efdcc2695ee1",
        ),
        (
            137,
            "f8d6846cedd2ccfadf15c5879ef95af724d799eed7391fb1c91f95344e738614",
        ),
        (
            200,
            "cce34485baf2bf2aca99b94833892a4f52896d3d153f7b840cc4f9fe695f1387",
        ),
        (
            271,
            "e79e5c6fef1bb5fdea2717ca27e88399e9b64699d1b3eb8e30f314fa055214e8",
        ),
        (
            272,
            "a490357b9b3fb39d0a89a117734e5b020b1f33c7bf3fa3575c396425432003d3",
        ),
        (
            1_000_000,
            "5c8875ae474a3634ba4fd55ec85bffd661f32aca75c6d699d0cdcb6c115891c1",
        ),
    ];
    for (len, want) in cases {
        assert_eq!(hex(&sha3_256(&vec![b'a'; len])), want, "{len}");
    }
}

/// The key and address of the example in the Tor rendezvous specification.
const SPEC_KEY: &str = "79bcc625184b05194975c28b66b66b0469f7f6556fb1ac3189a79b40dda32f1f";
const SPEC_NAME: &str = "pg6mmjiyjmcrsslvykfwnntlaru7p5svn6y2ymmju6nubxndf4pscryd.onion";

fn spec_key() -> [u8; 32] {
    unhex(SPEC_KEY).try_into().unwrap()
}

#[test]
fn nr_92_the_name_of_the_tor_specification_example_is_written_exactly() {
    assert_eq!(onion_name(&spec_key()), SPEC_NAME);
    assert_eq!(SPEC_NAME.len(), 56 + ".onion".len());
}

#[test]
fn nr_92_a_second_known_address_is_written_exactly() {
    let key = unhex("1d04a1d04a338c6e6ae970bfabee49049d6702250984ca950c01673f4ec034ad");
    assert_eq!(
        onion_name(&key.try_into().unwrap()),
        "duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion"
    );
}

#[test]
fn nr_92_a_name_reads_back_to_its_key() {
    assert_eq!(parse_onion_name(SPEC_NAME), Some(spec_key()));
    for seed in 0..64u8 {
        let mut key = [0u8; 32];
        for (i, b) in key.iter_mut().enumerate() {
            *b = seed.wrapping_mul(37).wrapping_add(i as u8).wrapping_mul(11);
        }
        let name = onion_name(&key);
        assert_eq!(name.len(), 62);
        assert!(name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || (b'2'..=b'7').contains(&c) || c == b'.'));
        assert_eq!(parse_onion_name(&name), Some(key));
    }
}

#[test]
fn nr_92_a_name_that_is_not_exactly_the_form_is_refused() {
    let base = &SPEC_NAME[..56];
    assert_eq!(parse_onion_name(base), None);
    assert_eq!(parse_onion_name(&format!("{base}.com")), None);
    assert_eq!(parse_onion_name(&format!("{}.onion", &base[..55])), None);
    assert_eq!(parse_onion_name(&format!("{base}a.onion")), None);
    assert_eq!(parse_onion_name(&SPEC_NAME.to_uppercase()), None);
    assert_eq!(parse_onion_name("seed.bitcoin.sipa.be"), None);
    assert_eq!(parse_onion_name(""), None);
    // A character outside the alphabet, and a wrong checksum.
    let bad_char = format!("1{}", &SPEC_NAME[1..]);
    assert_eq!(parse_onion_name(&bad_char), None);
    let mut flipped = SPEC_NAME.as_bytes().to_vec();
    flipped[10] = if flipped[10] == b'a' { b'b' } else { b'a' };
    assert_eq!(parse_onion_name(&String::from_utf8(flipped).unwrap()), None);
    // A wrong version byte: the last character of the base32 part.
    let mut other = SPEC_NAME.as_bytes().to_vec();
    other[55] = if other[55] == b'a' { b'b' } else { b'a' };
    assert_eq!(parse_onion_name(&String::from_utf8(other).unwrap()), None);
}

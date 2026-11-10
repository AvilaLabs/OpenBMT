//! LK-01 to LK-09: sealing and opening.

use std::cell::Cell;
use std::convert::Infallible;

use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::SecretKey;
use openbmt_core::identity::DeviceId;

use super::kit::*;
use crate::hkdf;
use crate::seal::{
    build, inspect, open, seal, signed_bytes, step_for, Kind, OpenError, Role, SealError,
    AEAD_TAG_LEN, HEADER_LEN, MAX_CONTENT_LEN, PREFIX_LEN, SIZE_STEPS, TAG_LINK,
};

fn hexs(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn open_as_counter(sealed: &[u8]) -> Result<crate::seal::Opened, OpenError> {
    open(&counter(), Role::Counter, sealed, allowed, none_taken)
}

fn fixed_seal(content: &[u8]) -> Vec<u8> {
    seal(
        &owner(),
        Role::Owner,
        &counter().device_id(),
        Role::Counter,
        7,
        Kind::Command,
        content,
        &mut |b: &mut [u8; 32]| -> Result<(), Infallible> {
            *b = [0x42; 32];
            Ok(())
        },
    )
    .unwrap()
}

/// Change a sealed message the way a sender who holds the owner key could:
/// let `edit` change it, then make the hash and the signature fit again.
fn resign(sealed: &[u8], edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut m = sealed.to_vec();
    edit(&mut m);
    let ct_hash = sha256::Hash::hash(&m[HEADER_LEN..]).to_byte_array();
    m[PREFIX_LEN..PREFIX_LEN + 32].copy_from_slice(&ct_hash);
    let signed = signed_bytes(
        m[5..37].try_into().unwrap(),
        m[37..69].try_into().unwrap(),
        u64::from_le_bytes(m[69..77].try_into().unwrap()),
        m[77],
        m[78..111].try_into().unwrap(),
        &ct_hash,
    );
    let sig = owner().sign_tagged(TAG_LINK, &signed);
    m[PREFIX_LEN + 32..HEADER_LEN].copy_from_slice(&sig.0);
    m
}

fn hand_built(plain: &[u8]) -> Vec<u8> {
    build(
        &owner(),
        &counter().device_id(),
        1,
        Kind::Command,
        plain,
        SecretKey::from_slice(&[0x41; 32]).unwrap(),
    )
    .unwrap()
}

#[test]
fn lk_01_a_message_names_one_reader_and_one_sender() {
    let sealed = owner_to_counter(1, Kind::Command, b"hello");
    assert_eq!(&sealed[5..37], &counter().device_id().to_bytes());
    assert_eq!(&sealed[37..69], &owner().device_id().to_bytes());
    let o = open_as_counter(&sealed).unwrap();
    assert_eq!(o.sender, owner().device_id());
    assert_eq!(o.content, b"hello");
}

#[test]
fn lk_01_a_message_sealed_to_one_device_does_not_open_for_another() {
    let sealed = owner_to_counter(1, Kind::Command, b"hello");
    // The box is on the list and has a key; the message is not for it.
    let r = open(&the_box(), Role::Box, &sealed, allowed, none_taken);
    assert_eq!(r.unwrap_err(), OpenError::WrongReader);
    // Even with the reader's field changed to the box's key (and the
    // signature made good again), the key derivation does not match.
    let moved = resign(&sealed, |m| {
        m[5..37].copy_from_slice(&the_box().device_id().to_bytes())
    });
    let r = open(&the_box(), Role::Box, &moved, allowed, none_taken);
    assert_eq!(r.unwrap_err(), OpenError::KindNotAllowed(Kind::Command));
    let other = seal(
        &owner(),
        Role::Owner,
        &the_box().device_id(),
        Role::Box,
        1,
        Kind::Settings,
        b"x",
        &mut |b: &mut [u8; 32]| -> Result<(), Infallible> {
            *b = [0x41; 32];
            Ok(())
        },
    )
    .unwrap();
    let moved = resign(&other, |m| {
        m[5..37].copy_from_slice(&counter().device_id().to_bytes())
    });
    // For the counter now, with the right signature: the key is wrong.
    let r = open_as_counter(&moved);
    assert_eq!(r.unwrap_err(), OpenError::Decrypt);
}

#[test]
fn lk_02_known_answer() {
    let sealed = fixed_seal(b"known answer");
    assert_eq!(sealed.len(), HEADER_LEN + 256 + AEAD_TAG_LEN);
    // Pinned from this implementation's own first run (it has no outside
    // source). A change to the format, the key
    // derivation, the nonce, the padding or the signature changes this.
    let digest = sha256::Hash::hash(&sealed).to_byte_array();
    assert_eq!(
        to_hex(&digest),
        "ed2d9063d378488882bcd89af8a95fa6626aabd1774d09cadbc6bb8eb535763b",
        "sealed bytes: {}",
        to_hex(&sealed)
    );
    let o = open_as_counter(&sealed).unwrap();
    assert_eq!(o.content, b"known answer");
    assert_eq!(o.number, 7);
}

#[test]
fn lk_02_hkdf_rfc5869_vector_1() {
    let ikm = [0x0b; 22];
    let salt = hexs("000102030405060708090a0b0c");
    let info = hexs("f0f1f2f3f4f5f6f7f8f9");
    let prk = hkdf::extract(&salt, &ikm);
    assert_eq!(
        to_hex(&prk),
        "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5"
    );
    let mut okm = [0u8; 42];
    assert!(hkdf::expand(&prk, &info, &mut okm));
    assert_eq!(
        to_hex(&okm),
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
    );
}

#[test]
fn lk_02_hkdf_rfc5869_vector_2() {
    let ikm: Vec<u8> = (0x00..=0x4f).collect();
    let salt: Vec<u8> = (0x60..=0xaf).collect();
    let info: Vec<u8> = (0xb0..=0xff).collect();
    let prk = hkdf::extract(&salt, &ikm);
    assert_eq!(
        to_hex(&prk),
        "06a6b88c5853361a06104c9ceb35b45cef760014904671014a193f40c15fc244"
    );
    let mut okm = [0u8; 82];
    assert!(hkdf::expand(&prk, &info, &mut okm));
    assert_eq!(
        to_hex(&okm),
        "b11e398dc80327a1c8e7f78c596a49344f012eda2d4efad8a050cc4c19afa97c\
         59045a99cac7827271cb41c65e590e09da3275600c2f09b8367793a9aca3db71\
         cc30c58179ec3e87c14c01d5c1f3434f1d87"
    );
}

#[test]
fn lk_02_hkdf_rfc5869_vector_3() {
    let ikm = [0x0b; 22];
    let prk = hkdf::extract(&[], &ikm);
    assert_eq!(
        to_hex(&prk),
        "19ef24a32c717b167f33a91d6f648bdf96596776afdb6377ac434c1c293ccb04"
    );
    let mut okm = [0u8; 42];
    assert!(hkdf::expand(&prk, &[], &mut okm));
    assert_eq!(
        to_hex(&okm),
        "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"
    );
    assert!(!hkdf::expand(
        &prk,
        &[],
        &mut vec![0u8; hkdf::MAX_OUTPUT + 1]
    ));
}

#[test]
fn lk_02_header_and_ciphertext_are_bound_to_each_other() {
    let sealed = owner_to_counter(5, Kind::Command, b"bound");
    // A sender with the owner's key changes the number in the header and
    // signs again: the signature is good, but the AEAD binds the header.
    let renumbered = resign(&sealed, |m| m[69..77].copy_from_slice(&9u64.to_le_bytes()));
    assert_eq!(
        open_as_counter(&renumbered).unwrap_err(),
        OpenError::Decrypt
    );
    // The same for the kind byte.
    let rekinded = resign(&sealed, |m| m[77] = Kind::Heartbeat.to_byte());
    assert_eq!(open_as_counter(&rekinded).unwrap_err(), OpenError::Decrypt);
    // A ciphertext byte changed and signed again does not open.
    let flipped = resign(&sealed, |m| m[HEADER_LEN] ^= 1);
    assert_eq!(open_as_counter(&flipped).unwrap_err(), OpenError::Decrypt);
}

#[test]
fn lk_02_two_seals_of_the_same_content_differ() {
    let a = fixed_seal(b"same");
    let mut rng = Counting(9);
    let b = seal(
        &owner(),
        Role::Owner,
        &counter().device_id(),
        Role::Counter,
        7,
        Kind::Command,
        b"same",
        &mut |x| rng.fill(x),
    )
    .unwrap();
    assert_ne!(a, b);
    // Different one-time key, so different ciphertext and different key.
    assert_ne!(&a[78..111], &b[78..111]);
    assert_ne!(&a[HEADER_LEN..], &b[HEADER_LEN..]);
}

#[test]
fn lk_03_failing_randomness_fails_the_seal() {
    let r = seal(
        &owner(),
        Role::Owner,
        &counter().device_id(),
        Role::Counter,
        1,
        Kind::Command,
        b"x",
        &mut |_: &mut [u8; 32]| -> Result<(), &'static str> { Err("no entropy") },
    );
    assert_eq!(r.unwrap_err(), SealError::Randomness);
}

#[test]
fn lk_03_randomness_that_is_not_a_key_fails_the_seal() {
    for bad in [[0u8; 32], [0xff; 32]] {
        let r = seal(
            &owner(),
            Role::Owner,
            &counter().device_id(),
            Role::Counter,
            1,
            Kind::Command,
            b"x",
            &mut |b: &mut [u8; 32]| -> Result<(), Infallible> {
                *b = bad;
                Ok(())
            },
        );
        assert_eq!(r.unwrap_err(), SealError::BadOneTimeKey);
    }
}

#[test]
fn lk_03_the_seal_asks_for_new_randomness_each_time() {
    let calls = Cell::new(0u32);
    for n in 1..=3 {
        seal(
            &owner(),
            Role::Owner,
            &counter().device_id(),
            Role::Counter,
            n,
            Kind::Command,
            b"x",
            &mut |b: &mut [u8; 32]| -> Result<(), Infallible> {
                calls.set(calls.get() + 1);
                *b = [calls.get() as u8; 32];
                Ok(())
            },
        )
        .unwrap();
    }
    assert_eq!(calls.get(), 3);
}

#[test]
fn lk_04_every_single_bit_flip_in_the_smallest_step_is_refused() {
    let sealed = owner_to_counter(1, Kind::Command, b"smallest step");
    assert_eq!(sealed.len(), HEADER_LEN + 256 + AEAD_TAG_LEN);
    assert!(open_as_counter(&sealed).is_ok());
    for byte in 0..sealed.len() {
        for bit in 0..8 {
            let mut m = sealed.clone();
            m[byte] ^= 1 << bit;
            assert!(
                open_as_counter(&m).is_err(),
                "flip of bit {bit} of byte {byte} was accepted"
            );
        }
    }
}

#[test]
fn lk_04_a_valid_signature_on_a_swapped_ciphertext_is_refused() {
    let a = owner_to_counter(1, Kind::Command, b"first");
    let b = owner_to_counter(2, Kind::Command, b"second");
    // Message A's header and signature, message B's ciphertext. Both are
    // validly signed by the same sender; only the hash tells them apart.
    let mut swapped = a[..HEADER_LEN].to_vec();
    swapped.extend_from_slice(&b[HEADER_LEN..]);
    assert_eq!(
        open_as_counter(&swapped).unwrap_err(),
        OpenError::CiphertextHash
    );
    assert_eq!(inspect(&swapped).unwrap_err(), OpenError::CiphertextHash);
}

#[test]
fn lk_04_the_signature_is_the_senders_and_a_relay_can_check_it() {
    let sealed = owner_to_counter(1, Kind::Command, b"x");
    let seen = inspect(&sealed).unwrap();
    assert_eq!(seen.reader, counter().device_id());
    assert_eq!(seen.sender, owner().device_id());
    assert_eq!(seen.kind, Kind::Command);
    assert_eq!(seen.step, 256);
    // Signed by someone else under the owner's name.
    let forged = {
        let mut m = sealed.clone();
        let signed = signed_bytes(
            m[5..37].try_into().unwrap(),
            m[37..69].try_into().unwrap(),
            1,
            m[77],
            m[78..111].try_into().unwrap(),
            m[PREFIX_LEN..PREFIX_LEN + 32].try_into().unwrap(),
        );
        let sig = stranger().sign_tagged(TAG_LINK, &signed);
        m[PREFIX_LEN + 32..HEADER_LEN].copy_from_slice(&sig.0);
        m
    };
    assert_eq!(inspect(&forged).unwrap_err(), OpenError::BadSignature);
}

#[test]
fn lk_05_checks_run_in_the_stated_order() {
    let sealed = owner_to_counter(5, Kind::Command, b"x");

    // 1. The frame, before the list is asked.
    let asked = Cell::new(false);
    let r = open(
        &counter(),
        Role::Counter,
        &sealed[..sealed.len() - 1],
        |d| {
            asked.set(true);
            allowed(d)
        },
        none_taken,
    );
    assert!(matches!(r, Err(OpenError::Frame(_))));
    assert!(!asked.get());
    let mut bad_magic = sealed.clone();
    bad_magic[0] = b'X';
    assert!(matches!(
        open_as_counter(&bad_magic),
        Err(OpenError::Frame(_))
    ));

    // 2. The reader, before the list.
    let r = open(&the_box(), Role::Box, &sealed, |_| None, none_taken);
    assert_eq!(r.unwrap_err(), OpenError::WrongReader);

    // 3. The list, before the kind.
    let r = open(&counter(), Role::Counter, &sealed, |_| None, none_taken);
    assert_eq!(r.unwrap_err(), OpenError::SenderNotListed);

    // 4. The kind, before the signature.
    let mut bad_sig = sealed.clone();
    bad_sig[PREFIX_LEN + 32] ^= 1;
    let r = open(
        &counter(),
        Role::Counter,
        &bad_sig,
        |_| Some(Role::Counter),
        none_taken,
    );
    assert_eq!(r.unwrap_err(), OpenError::KindNotAllowed(Kind::Command));

    // 5. The signature, before the number.
    let r = open(&counter(), Role::Counter, &bad_sig, allowed, |_| 99);
    assert_eq!(r.unwrap_err(), OpenError::BadSignature);

    // 6. The number, before the decryption: a signed message that would
    // fail to decrypt is a replay first when its number is old.
    let undecryptable = resign(&sealed, |m| m[HEADER_LEN] ^= 1);
    let r = open(&counter(), Role::Counter, &undecryptable, allowed, |_| 5);
    assert_eq!(r.unwrap_err(), OpenError::Replay { last: 5, got: 5 });
    assert_eq!(
        open_as_counter(&undecryptable).unwrap_err(),
        OpenError::Decrypt
    );

    // 7. The decryption, before the padding.
    // 8. The padding.
    let mut plain = vec![0u8; 256];
    plain[..4].copy_from_slice(&1u32.to_le_bytes());
    plain[4] = 9;
    plain[100] = 1;
    assert_eq!(
        open_as_counter(&hand_built(&plain)).unwrap_err(),
        OpenError::Padding
    );
}

#[test]
fn lk_06_a_number_at_or_below_the_last_is_a_replay_and_gaps_are_allowed() {
    let sealed = owner_to_counter(5, Kind::Command, b"x");
    let r = open(&counter(), Role::Counter, &sealed, allowed, |_| 5);
    assert_eq!(r.unwrap_err(), OpenError::Replay { last: 5, got: 5 });
    let r = open(&counter(), Role::Counter, &sealed, allowed, |_| 9);
    assert_eq!(r.unwrap_err(), OpenError::Replay { last: 9, got: 5 });
    let r = open(&counter(), Role::Counter, &sealed, allowed, |_| 4);
    assert_eq!(r.unwrap().number, 5);
    // A big gap is fine.
    let r = open(&counter(), Role::Counter, &sealed, allowed, |_| 0);
    assert!(r.is_ok());
    // Number 0 is never sealed.
    let mut rng = Counting(0);
    let r = seal(
        &owner(),
        Role::Owner,
        &counter().device_id(),
        Role::Counter,
        0,
        Kind::Command,
        b"x",
        &mut |b| rng.fill(b),
    );
    assert_eq!(r.unwrap_err(), SealError::NumberZero);
}

#[test]
fn lk_07_padding_steps_at_every_boundary() {
    let cases: [(usize, usize); 11] = [
        (0, 256),
        (1, 256),
        (252, 256),
        (253, 1024),
        (1020, 1024),
        (1021, 4096),
        (4092, 4096),
        (4093, 16384),
        (16380, 16384),
        (16381, 65536),
        (MAX_CONTENT_LEN, 65536),
    ];
    for (len, step) in cases {
        assert_eq!(step_for(len), Some(step), "len {len}");
        let content = vec![0xa5u8; len];
        let sealed = owner_to_counter(1, Kind::Batch, &content);
        assert_eq!(sealed.len(), HEADER_LEN + step + AEAD_TAG_LEN, "len {len}");
        assert_eq!(inspect(&sealed).unwrap().step, step);
        let o = open_as_counter(&sealed).unwrap();
        assert_eq!(o.content, content, "len {len}");
    }
    assert_eq!(SIZE_STEPS, [256, 1024, 4096, 16384, 65536]);
    assert_eq!(step_for(MAX_CONTENT_LEN + 1), None);
}

#[test]
fn lk_07_larger_content_is_refused() {
    let mut rng = Counting(0);
    let r = seal(
        &owner(),
        Role::Owner,
        &counter().device_id(),
        Role::Counter,
        1,
        Kind::Batch,
        &vec![0u8; MAX_CONTENT_LEN + 1],
        &mut |b| rng.fill(b),
    );
    assert_eq!(r.unwrap_err(), SealError::TooLarge);
}

#[test]
fn lk_07_padding_that_is_not_zero_or_not_minimal_is_refused() {
    let plain_with = |len: u32, size: usize, edit: &dyn Fn(&mut Vec<u8>)| {
        let mut p = vec![0u8; size];
        p[..4].copy_from_slice(&len.to_le_bytes());
        edit(&mut p);
        hand_built(&p)
    };
    // Well formed by hand is accepted: the internal builder is not the
    // problem.
    assert!(open_as_counter(&plain_with(3, 256, &|_| {})).is_ok());
    // A nonzero padding byte, first and last.
    for at in [7usize, 255] {
        let m = plain_with(3, 256, &|p| p[at] = 1);
        assert_eq!(
            open_as_counter(&m).unwrap_err(),
            OpenError::Padding,
            "at {at}"
        );
    }
    // A length longer than the plaintext.
    let m = plain_with(253, 256, &|_| {});
    assert_eq!(open_as_counter(&m).unwrap_err(), OpenError::Padding);
    let m = plain_with(u32::MAX, 256, &|_| {});
    assert_eq!(open_as_counter(&m).unwrap_err(), OpenError::Padding);
    // A short content in a larger step than needed.
    let m = plain_with(3, 1024, &|_| {});
    assert_eq!(open_as_counter(&m).unwrap_err(), OpenError::Padding);
}

#[test]
fn lk_08_the_table_of_kinds() {
    use Role::{Box as B, Counter as C, Owner as O};
    let table: [(Kind, Role, Role); 12] = [
        (Kind::Heartbeat, O, C),
        (Kind::Command, O, C),
        (Kind::Settings, O, C),
        (Kind::Settings, O, B),
        (Kind::Batch, O, C),
        (Kind::DeviceList, O, C),
        (Kind::DeviceList, O, B),
        (Kind::CheckIn, C, O),
        (Kind::Records, C, O),
        (Kind::PaidNotice, C, B),
        (Kind::PosNotice, B, C),
        (Kind::PosNotice, B, O),
    ];
    for kind in Kind::ALL {
        for from in [O, C, B] {
            for to in [O, C, B] {
                let want = table.contains(&(kind, from, to));
                assert_eq!(kind.allowed(from, to), want, "{kind:?} {from:?} {to:?}");
            }
        }
        assert_eq!(Kind::from_byte(kind.to_byte()), Some(kind));
    }
}

#[test]
fn lk_08_seal_and_open_refuse_a_kind_the_role_may_not_send() {
    let mut rng = Counting(0);
    // The counter may not send a command, the box may not send a heartbeat.
    let r = seal(
        &counter(),
        Role::Counter,
        &owner().device_id(),
        Role::Owner,
        1,
        Kind::Command,
        b"x",
        &mut |b| rng.fill(b),
    );
    assert_eq!(r.unwrap_err(), SealError::KindNotAllowed);
    // Open: the list says the sender is the counter, the message is a
    // command (sealed by someone holding the counter's key, by hand).
    let m = hand_signed_by(&counter(), &owner(), Kind::Command);
    let r = open(&owner(), Role::Owner, &m, allowed, none_taken);
    assert_eq!(r.unwrap_err(), OpenError::KindNotAllowed(Kind::Command));
    // And for a role the sender does not have: the owner's key listed as a box.
    let m = owner_to_counter(1, Kind::Command, b"x");
    let r = open(
        &counter(),
        Role::Counter,
        &m,
        |_| Some(Role::Box),
        none_taken,
    );
    assert_eq!(r.unwrap_err(), OpenError::KindNotAllowed(Kind::Command));
    // An unknown kind byte.
    let mut unknown = owner_to_counter(1, Kind::Command, b"x");
    unknown[77] = 0;
    assert_eq!(
        open_as_counter(&unknown).unwrap_err(),
        OpenError::UnknownKind(0)
    );
    unknown[77] = 10;
    assert_eq!(inspect(&unknown).unwrap_err(), OpenError::UnknownKind(10));
}

/// A message `from` seals to `to` with a kind the table would not allow,
/// built through the internal function.
fn hand_signed_by(
    from: &openbmt_core::identity::Identity,
    to: &openbmt_core::identity::Identity,
    kind: Kind,
) -> Vec<u8> {
    let mut plain = vec![0u8; 256];
    plain[..4].copy_from_slice(&1u32.to_le_bytes());
    build(
        from,
        &to.device_id(),
        1,
        kind,
        &plain,
        SecretKey::from_slice(&[0x41; 32]).unwrap(),
    )
    .unwrap()
}

#[test]
fn lk_09_a_message_from_the_box_is_never_a_heartbeat() {
    let mut rng = Counting(0);
    for to in [(Role::Counter, counter()), (Role::Owner, owner())] {
        let r = seal(
            &the_box(),
            Role::Box,
            &to.1.device_id(),
            to.0,
            1,
            Kind::Heartbeat,
            b"x",
            &mut |b| rng.fill(b),
        );
        assert_eq!(r.unwrap_err(), SealError::KindNotAllowed);
    }
    // And a box key that seals one by hand is refused by the reader.
    let m = hand_signed_by(&the_box(), &counter(), Kind::Heartbeat);
    let r = open_as_counter(&m);
    assert_eq!(r.unwrap_err(), OpenError::KindNotAllowed(Kind::Heartbeat));
    // What the box may send is a notice.
    let m = hand_signed_by(&the_box(), &counter(), Kind::PosNotice);
    assert_eq!(open_as_counter(&m).unwrap().kind, Kind::PosNotice);
}

#[test]
fn lk_05_a_stranger_is_refused() {
    let mut rng = Counting(0);
    let m = seal(
        &stranger(),
        Role::Owner,
        &counter().device_id(),
        Role::Counter,
        1,
        Kind::Command,
        b"x",
        &mut |b| rng.fill(b),
    )
    .unwrap();
    assert_eq!(open_as_counter(&m).unwrap_err(), OpenError::SenderNotListed);
    let _: DeviceId = stranger().device_id();
}

#[test]
fn lk_05_nothing_of_the_content_is_in_an_error() {
    let m = owner_to_counter(1, Kind::Command, b"SECRETCONTENT");
    let e = open(&counter(), Role::Counter, &m, allowed, |_| 9).unwrap_err();
    assert!(!format!("{e:?}{e}").contains("SECRET"));
}

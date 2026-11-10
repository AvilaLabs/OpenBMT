//! BK-30 and BK-31: hostile bytes, and the same books giving the same bytes.

use super::fixture::*;
use crate::sealed::seal_with;
use crate::{decode_export_header, open_export, Books, KdfParams, RandomFailed, RevealsPayments};

struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn bk_30_arbitrary_bytes_never_panic_a_decoder_and_what_decodes_encodes_back() {
    let valid = rich_kit().books().to_bundle();
    let mut rng = Xorshift(0x9e37_79b9_7f4a_7c15);
    for round in 0..4000 {
        let mut bytes = valid.clone();
        for _ in 0..=(rng.next() % 4) {
            let at = (rng.next() as usize) % bytes.len();
            bytes[at] = rng.next() as u8;
        }
        if round % 5 == 0 {
            let cut = (rng.next() as usize) % bytes.len();
            bytes.truncate(cut);
        }
        if round % 7 == 0 {
            bytes.extend((0..(rng.next() % 9)).map(|_| rng.next() as u8));
        }
        if let Ok(books) = Books::from_bundle(&bytes) {
            assert_eq!(books.to_bundle(), bytes);
        }
        let _ = decode_export_header(&bytes);
    }
    for len in 0..200 {
        let noise: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        let _ = Books::from_bundle(&noise);
        let _ = decode_export_header(&noise);
        let _ = open_export("a passphrase", &noise);
    }
}

#[test]
fn bk_30_every_byte_of_a_valid_bundle_changed_never_panics() {
    let valid = rich_kit().books().to_bundle();
    for at in 0..valid.len() {
        for value in [0u8, 1, 0x7f, 0xff] {
            let mut bytes = valid.clone();
            bytes[at] = value;
            if let Ok(books) = Books::from_bundle(&bytes) {
                assert_eq!(books.to_bundle(), bytes);
            }
        }
    }
}

#[test]
fn bk_31_the_same_books_give_the_same_csv_and_bundle() {
    let a = rich_kit().books();
    let b = rich_kit().books();
    assert_eq!(a, b);
    assert_eq!(a.to_bundle(), b.to_bundle());
    assert_eq!(
        a.full_ledger(RevealsPayments::Acknowledged),
        b.full_ledger(RevealsPayments::Acknowledged)
    );
    assert_eq!(a.accounting_export(), b.accounting_export());
}

#[test]
fn bk_31_only_the_salt_differs_from_one_encrypted_file_to_the_next() {
    let books = rich_kit().books();
    let small = KdfParams {
        m_kib: 64,
        t: 1,
        p: 1,
    };
    let salted = |byte: u8| {
        seal_with(
            &books,
            "pass phrase here",
            9,
            small,
            move |s: &mut [u8; 16]| {
                *s = [byte; 16];
                Ok::<(), RandomFailed>(())
            },
        )
        .expect("sealed")
    };
    assert_eq!(salted(1), salted(1));
    assert_ne!(salted(1), salted(2));
}

#[test]
fn bk_30_the_two_counters_build_in_any_order_to_the_same_books() {
    let a = kit_with(&[opened(0, 10, 100)]);
    let other = identity(3);
    let mut chain_b = openbmt_core::records::Chain::new();
    write(&mut chain_b, &other, &opened(0, 20, 200));
    let devices = openbmt_core::records::DeviceList::new([
        a.counter_id(),
        other.device_id(),
        a.owner.device_id(),
    ]);
    let one = Books::from_chains(
        &devices,
        &[(a.counter_id(), &a.chain), (other.device_id(), &chain_b)],
    )
    .expect("books");
    let two = Books::from_chains(
        &devices,
        &[(other.device_id(), &chain_b), (a.counter_id(), &a.chain)],
    )
    .expect("books");
    assert_eq!(one, two);
    assert_eq!(one.sales().len(), 2);
    assert_eq!(one.high_water().len(), 2);
}

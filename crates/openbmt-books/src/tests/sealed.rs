//! BK-23 to BK-29: the encrypted export file, the bundle and the file helper.

use std::time::Instant;

use super::fixture::*;
use super::outflow::fee;
use crate::sealed::seal_with;
use crate::{
    decode_export_header, open_export, seal_export, write_whole, Books, CashReturn, Disposal,
    DisposalKind, KdfParams, OpenError, PosFacts, RandomFailed, SealError, MAGIC_BUNDLE,
    MAGIC_EXPORT,
};

const SMALL: KdfParams = KdfParams {
    m_kib: 64,
    t: 1,
    p: 1,
};
const PASS: &str = "correct horse battery";

fn salt_of(byte: u8) -> impl FnOnce(&mut [u8; 16]) -> Result<(), RandomFailed> {
    move |s| {
        *s = [byte; 16];
        Ok(())
    }
}

/// Books with every kind of line.
fn full_books() -> Books {
    let kit = rich_kit();
    let mut facts = PosFacts::new();
    facts.insert_tax(kit.key(0), 148);
    kit.books()
        .with_pos_facts(&facts)
        .expect("facts")
        .with_disposals(vec![
            Disposal {
                at: FEB_2_NOON + 10,
                kind: DisposalKind::Refund { sale: kit.key(0) },
                currency: usd(),
                sats: SATS_REFUND,
                minor_value: Some(500),
                basis: Some(crate::Basis { minor: 450 }),
            },
            fee(FEB_2_NOON + 20, 9, None),
        ])
        .expect("disposals")
        .with_cash_returns(vec![CashReturn {
            at: FEB_2_NOON + 30,
            sale: kit.key(5),
            minor: 100,
        }])
        .expect("returns")
}

fn sealed() -> (Books, Vec<u8>) {
    let books = full_books();
    let file = seal_with(&books, PASS, 1_738_500_000, SMALL, salt_of(7)).expect("sealed");
    (books, file)
}

#[test]
fn bk_23_the_header_is_readable_without_the_passphrase() {
    let (books, file) = sealed();
    assert_eq!(file.get(..4), Some(&MAGIC_EXPORT[..]));
    assert_eq!(file.get(4), Some(&1));
    let h = decode_export_header(&file).expect("header");
    assert_eq!(h.params, SMALL);
    assert_eq!(h.salt, [7; 16]);
    assert_eq!(h.created_at, 1_738_500_000);
    assert_eq!(&h.high_water, books.high_water());
    assert_eq!(
        h.to_bytes().as_slice(),
        file.get(..h.to_bytes().len()).expect("prefix")
    );
}

#[test]
fn bk_23_reading_is_strict_about_magic_version_and_lengths() {
    let (_, file) = sealed();
    let mut bad = file.clone();
    bad[0] = b'X';
    assert!(decode_export_header(&bad).is_err());
    let mut bad = file.clone();
    bad[4] = 2;
    assert!(decode_export_header(&bad).is_err());
    for cut in 0..file.len() {
        assert!(open_export(PASS, &file[..cut]).is_err(), "cut at {cut}");
    }
    // A header with no ciphertext after it is refused.
    let h = decode_export_header(&file).expect("header");
    let header_only = file[..h.to_bytes().len()].to_vec();
    assert!(decode_export_header(&header_only).is_err());
}

#[test]
fn bk_24_the_standard_parameters_are_64_mib_3_passes_1_lane() {
    assert_eq!(
        KdfParams::STANDARD,
        KdfParams {
            m_kib: 64 * 1024,
            t: 3,
            p: 1
        }
    );
    assert_eq!(
        KdfParams::LIMIT,
        KdfParams {
            m_kib: 256 * 1024,
            t: 10,
            p: 4
        }
    );
}

#[test]
fn bk_24_a_file_made_with_the_real_parameters_opens() {
    let books = full_books();
    let file = seal_export(&books, PASS, 5, salt_of(1)).expect("sealed");
    let h = decode_export_header(&file).expect("header");
    assert_eq!(h.params, KdfParams::STANDARD);
    assert_eq!(open_export(PASS, &file), Ok(books));
}

#[test]
fn bk_24_the_key_depends_on_the_passphrase_and_the_salt() {
    let (_, a) = sealed();
    let books = full_books();
    let b = seal_with(&books, PASS, 1_738_500_000, SMALL, salt_of(8)).expect("sealed");
    let c = seal_with(
        &books,
        "another passphrase",
        1_738_500_000,
        SMALL,
        salt_of(7),
    )
    .expect("sealed");
    let body =
        |f: &Vec<u8>| f[decode_export_header(f).expect("header").to_bytes().len()..].to_vec();
    assert_ne!(body(&a), body(&c));
    assert_ne!(body(&a), body(&b));
}

#[test]
fn bk_25_the_salt_comes_from_the_callers_randomness_and_failure_makes_no_file() {
    let books = full_books();
    let failing = |_: &mut [u8; 16]| Err(RandomFailed);
    assert_eq!(
        seal_with(&books, PASS, 1, SMALL, failing),
        Err(SealError::Randomness)
    );
    assert_eq!(
        seal_export(&books, PASS, 1, failing),
        Err(SealError::Randomness)
    );
    let one = seal_with(&books, PASS, 1, SMALL, salt_of(1)).expect("sealed");
    let two = seal_with(&books, PASS, 1, SMALL, salt_of(2)).expect("sealed");
    assert_eq!(decode_export_header(&one).expect("h").salt, [1; 16]);
    assert_ne!(one, two);
}

#[test]
fn bk_26_the_books_come_back_and_a_wrong_passphrase_gives_one_error() {
    let (books, file) = sealed();
    assert_eq!(open_export(PASS, &file), Ok(books));
    let wrong = open_export("not the passphrase", &file);
    assert_eq!(wrong, Err(OpenError::WrongPassphraseOrDamaged));
    assert_eq!(
        wrong.expect_err("error").to_string(),
        "BK-26: wrong passphrase or damaged file"
    );
}

#[test]
fn bk_26_one_flipped_byte_anywhere_in_the_file_is_refused() {
    let (_, file) = sealed();
    for at in 0..file.len() {
        let mut bad = file.clone();
        bad[at] ^= 0x01;
        assert_eq!(
            open_export(PASS, &bad),
            Err(OpenError::WrongPassphraseOrDamaged),
            "byte {at}"
        );
    }
}

#[test]
fn bk_26_a_truncated_or_extended_file_is_refused() {
    let (_, file) = sealed();
    assert!(open_export(PASS, &file[..file.len() - 1]).is_err());
    let mut longer = file.clone();
    longer.push(0);
    assert!(open_export(PASS, &longer).is_err());
    assert!(open_export(PASS, &[]).is_err());
}

#[test]
fn bk_26_the_header_is_the_associated_data() {
    // A header with another creation time, over the same ciphertext.
    let (_, file) = sealed();
    let h = decode_export_header(&file).expect("header");
    let mut other = h.clone();
    other.created_at += 1;
    let mut forged = other.to_bytes();
    forged.extend_from_slice(&file[h.to_bytes().len()..]);
    assert_eq!(
        open_export(PASS, &forged),
        Err(OpenError::WrongPassphraseOrDamaged)
    );
}

fn header_with(file: &[u8], m: u32, t: u32, p: u32) -> Vec<u8> {
    let mut out = file.to_vec();
    out[5..9].copy_from_slice(&m.to_le_bytes());
    out[9..13].copy_from_slice(&t.to_le_bytes());
    out[13..17].copy_from_slice(&p.to_le_bytes());
    out
}

#[test]
fn bk_27_parameters_above_the_limit_are_refused_before_argon2_runs() {
    let (_, file) = sealed();
    let limit = KdfParams::LIMIT;
    let started = Instant::now();
    for (m, t, p) in [
        (limit.m_kib + 1, 1, 1),
        (64, limit.t + 1, 1),
        (64, 1, limit.p + 1),
        (u32::MAX, u32::MAX, u32::MAX),
    ] {
        let hostile = header_with(&file, m, t, p);
        assert!(decode_export_header(&hostile).is_err());
        assert_eq!(
            open_export(PASS, &hostile),
            Err(OpenError::WrongPassphraseOrDamaged)
        );
    }
    // Had Argon2 run at the largest of these, it would take far longer.
    assert!(started.elapsed().as_secs() < 5);
    // Parameters at the limit are not refused by the header.
    let at_limit = header_with(&file, limit.m_kib, limit.t, limit.p);
    assert!(decode_export_header(&at_limit).is_ok());
    // Zero and too little memory for the lanes are refused too.
    for (m, t, p) in [(64, 0, 1), (64, 1, 0), (7, 1, 1), (31, 1, 4)] {
        assert!(decode_export_header(&header_with(&file, m, t, p)).is_err());
    }
}

#[test]
fn bk_27_an_empty_passphrase_is_refused() {
    let books = full_books();
    assert_eq!(
        seal_with(&books, "", 1, SMALL, salt_of(1)),
        Err(SealError::EmptyPassphrase)
    );
    let (_, file) = sealed();
    assert_eq!(open_export("", &file), Err(OpenError::EmptyPassphrase));
}

#[test]
fn bk_27_sealing_refuses_parameters_above_the_limit() {
    let books = full_books();
    let too_big = KdfParams {
        m_kib: KdfParams::LIMIT.m_kib + 1,
        ..SMALL
    };
    assert_eq!(
        seal_with(&books, PASS, 1, too_big, salt_of(1)),
        Err(SealError::Key)
    );
}

#[test]
fn bk_28_the_bundle_round_trips_every_field() {
    let books = full_books();
    let bytes = books.to_bundle();
    assert_eq!(bytes.get(..4), Some(&MAGIC_BUNDLE[..]));
    let back = Books::from_bundle(&bytes).expect("bundle");
    assert_eq!(back, books);
    assert_eq!(back.to_bundle(), bytes);
    let empty = Books::default();
    assert_eq!(Books::from_bundle(&empty.to_bundle()), Ok(empty));
}

#[test]
fn bk_28_the_bundle_is_read_strictly() {
    let bytes = full_books().to_bundle();
    let mut bad = bytes.clone();
    bad[0] = b'Z';
    assert!(Books::from_bundle(&bad).is_err());
    let mut bad = bytes.clone();
    bad[4] = 9;
    assert!(Books::from_bundle(&bad).is_err());
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(Books::from_bundle(&longer).is_err());
    for cut in 0..bytes.len() {
        assert!(Books::from_bundle(&bytes[..cut]).is_err(), "cut at {cut}");
    }
}

#[test]
fn bk_28_a_high_water_mark_that_is_not_the_sales_highest_index_is_refused() {
    let books = full_books();
    let bytes = books.to_bundle();
    // The one counter's mark: 4 (count) after the 5-byte frame, 32 key, flag,
    // then the index.
    let at = 5 + 4 + 32 + 1;
    assert_eq!(bytes[at], 7);
    let mut bad = bytes.clone();
    bad[at] = 6;
    assert!(Books::from_bundle(&bad).is_err());
    let mut bad = bytes.clone();
    bad[at] = 8;
    assert!(Books::from_bundle(&bad).is_err());
}

#[test]
fn bk_28_the_marks_inside_the_file_must_match_the_header() {
    // A file whose bundle is of other books than its header says.
    let books = full_books();
    let other = Books::default();
    let h = crate::ExportHeader {
        params: SMALL,
        salt: [3; 16],
        created_at: 1,
        high_water: other.high_water().clone(),
    };
    let sealed = seal_with(&books, PASS, 1, SMALL, salt_of(3)).expect("sealed");
    let cut = decode_export_header(&sealed).expect("h").to_bytes().len();
    let mut forged = h.to_bytes();
    forged.extend_from_slice(&sealed[cut..]);
    assert!(open_export(PASS, &forged).is_err());
}

fn scratch_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("openbmt-books-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

#[test]
fn bk_29_a_file_is_written_whole_with_no_temporary_left_behind() {
    let dir = scratch_dir("whole");
    let path = dir.join("books.obmy");
    write_whole(&path, b"first").expect("written");
    assert_eq!(std::fs::read(&path).expect("read"), b"first");
    write_whole(&path, b"second, longer").expect("written");
    assert_eq!(std::fs::read(&path).expect("read"), b"second, longer");
    write_whole(&path, b"x").expect("written");
    assert_eq!(std::fs::read(&path).expect("read"), b"x");
    let names: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("books.obmy")]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bk_29_a_failed_write_leaves_the_old_file_and_no_temporary() {
    let dir = scratch_dir("failed");
    let path = dir.join("books.obmy");
    write_whole(&path, b"old").expect("written");
    // The target is a folder: the rename fails.
    let blocked = dir.join("blocked");
    std::fs::create_dir(&blocked).expect("dir");
    assert!(write_whole(&blocked, b"new").is_err());
    assert_eq!(std::fs::read(&path).expect("read"), b"old");
    assert!(!dir.join("blocked.part").exists());
    assert!(write_whole(&dir.join("missing").join("f"), b"x").is_err());
    assert!(write_whole(std::path::Path::new("/"), b"x").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

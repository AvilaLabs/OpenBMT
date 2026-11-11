use crate::matrix::{
    balance_penalty, block_penalty, finder_like_penalty, penalty, run_penalty,
    with_error_correction,
};
use crate::{
    encode_bytes, encode_segment, encode_text, encode_with, format_bits, segment_bits, tables,
    version_bits, Ecc, Mode, QrError,
};

fn bits(s: &str) -> Vec<bool> {
    s.chars().map(|c| c == '1').collect()
}

fn bits_string(v: u32, n: u32) -> String {
    (0..n)
        .rev()
        .map(|i| if (v >> i) & 1 == 1 { '1' } else { '0' })
        .collect()
}

#[test]
fn qr_annex_01234567_at_1m() {
    // ISO/IEC 18004, the worked example: "01234567", version 1-M.
    let seg = segment_bits(Mode::Numeric, b"01234567", 1, 16 * 8);
    let mut data: Vec<u8> = seg
        .chunks(8)
        .map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | u8::from(b)))
        .collect();
    assert_eq!(data, [0x10, 0x20, 0x0C, 0x56, 0x61, 0x80]);
    let mut pad = [0xECu8, 0x11].into_iter().cycle();
    while data.len() < 16 {
        data.extend(pad.next());
    }
    assert_eq!(
        data,
        [
            0x10, 0x20, 0x0C, 0x56, 0x61, 0x80, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11,
            0xEC, 0x11
        ]
    );
    let all = with_error_correction(&data, 1, Ecc::M);
    assert_eq!(all.len(), 26);
    assert_eq!(all.get(..16), Some(&data[..]));
    assert_eq!(
        all.get(16..),
        Some(&[0xA5u8, 0x24, 0xD4, 0xC1, 0xED, 0x36, 0xC7, 0x87, 0x2C, 0x55][..])
    );
    let code = encode_text("01234567", Ecc::M).unwrap();
    assert_eq!((code.version(), code.mode()), (1, Mode::Numeric));
    assert_eq!(code.size(), 21);
}

#[test]
fn qr_format_table_all_32() {
    // ISO/IEC 18004, Table C.1, in the order of mask 0 to 7 for each level.
    let table: [(Ecc, [&str; 8]); 4] = [
        (
            Ecc::M,
            [
                "101010000010010",
                "101000100100101",
                "101111001111100",
                "101101101001011",
                "100010111111001",
                "100000011001110",
                "100111110010111",
                "100101010100000",
            ],
        ),
        (
            Ecc::L,
            [
                "111011111000100",
                "111001011110011",
                "111110110101010",
                "111100010011101",
                "110011000101111",
                "110001100011000",
                "110110001000001",
                "110100101110110",
            ],
        ),
        (
            Ecc::H,
            [
                "001011010001001",
                "001001110111110",
                "001110011100111",
                "001100111010000",
                "000011101100010",
                "000001001010101",
                "000110100001100",
                "000100000111011",
            ],
        ),
        (
            Ecc::Q,
            [
                "011010101011111",
                "011000001101000",
                "011111100110001",
                "011101000000110",
                "010010010110100",
                "010000110000011",
                "010111011011010",
                "010101111101101",
            ],
        ),
    ];
    let mut n = 0;
    for (ecc, row) in table {
        for (mask, want) in row.iter().enumerate() {
            let got = bits_string(u32::from(format_bits(ecc, mask as u8)), 15);
            assert_eq!(&got, want, "{ecc:?} mask {mask}");
            n += 1;
        }
    }
    assert_eq!(n, 32);
}

#[test]
fn qr_version_table_7_to_40() {
    // ISO/IEC 18004, Table D.1.
    let table: [u32; 34] = [
        0x07C94, 0x085BC, 0x09A99, 0x0A4D3, 0x0BBF6, 0x0C762, 0x0D847, 0x0E60D, 0x0F928, 0x10B78,
        0x1145D, 0x12A17, 0x13532, 0x149A6, 0x15683, 0x168C9, 0x177EC, 0x18EC4, 0x191E1, 0x1AFAB,
        0x1B08E, 0x1CC1A, 0x1D33F, 0x1ED75, 0x1F250, 0x209D5, 0x216F0, 0x228BA, 0x2379F, 0x24B0B,
        0x2542E, 0x26A64, 0x27541, 0x28C69,
    ];
    for (i, want) in table.iter().enumerate() {
        assert_eq!(version_bits(7 + i as u8), *want, "version {}", 7 + i);
    }
}

#[test]
fn qr_format_and_version_bits_are_in_the_matrix() {
    for (v, ecc, mask) in [
        (1u8, Ecc::L, 0u8),
        (5, Ecc::Q, 3),
        (7, Ecc::H, 7),
        (40, Ecc::M, 5),
    ] {
        let c = encode_with(b"HELLO", ecc, Some(v), Some(mask)).unwrap();
        let n = c.size();
        let f = u32::from(format_bits(ecc, mask));
        let bit = |i: u32| (f >> i) & 1 == 1;
        // First copy, around the top-left finder.
        let first: Vec<(usize, usize)> = (0..6)
            .map(|i| (8, i))
            .chain([(8, 7), (8, 8), (7, 8)])
            .chain((9..15).map(|i| (14 - i, 8)))
            .collect();
        for (i, (x, y)) in first.iter().enumerate() {
            assert_eq!(c.get(*x, *y), bit(i as u32), "first copy bit {i}");
        }
        // Second copy, split between the other two finders.
        for i in 0..8 {
            assert_eq!(c.get(n - 1 - i as usize, 8), bit(i));
        }
        for i in 8..15 {
            assert_eq!(c.get(8, n - 15 + i as usize), bit(i));
        }
        assert!(c.get(8, n - 8), "the dark module");
        if v >= 7 {
            let vb = version_bits(v);
            for i in 0..18usize {
                let want = (vb >> i) & 1 == 1;
                assert_eq!(c.get(n - 11 + i % 3, i / 3), want);
                assert_eq!(c.get(i / 3, n - 11 + i % 3), want);
            }
        }
    }
}

#[test]
fn qr_finders_and_timing() {
    let c = encode_text("HELLO WORLD", Ecc::Q).unwrap();
    let n = c.size();
    for (ox, oy) in [(0, 0), (n - 7, 0), (0, n - 7)] {
        for dy in 0..7 {
            for dx in 0..7 {
                let edge = dx == 0 || dx == 6 || dy == 0 || dy == 6;
                let core = (2..=4).contains(&dx) && (2..=4).contains(&dy);
                assert_eq!(c.get(ox + dx, oy + dy), edge || core);
            }
        }
    }
    for i in 8..n - 8 {
        assert_eq!(c.get(i, 6), i % 2 == 0);
        assert_eq!(c.get(6, i), i % 2 == 0);
    }
    // The separators are light.
    for i in 0..8 {
        assert!(!c.get(7, i) && !c.get(i, 7));
    }
}

#[test]
fn qr_capacity_table_known_values() {
    // Byte-mode capacity, ISO/IEC 18004 Table 7 (a sample of it).
    let known: [(u8, [usize; 4]); 6] = [
        (1, [17, 14, 11, 7]),
        (10, [271, 213, 151, 119]),
        (20, [858, 666, 482, 382]),
        (30, [1732, 1370, 982, 742]),
        (40, [2953, 2331, 1663, 1273]),
        (7, [154, 122, 86, 64]),
    ];
    for (v, caps) in known {
        for (ecc, cap) in Ecc::ALL.into_iter().zip(caps) {
            let c = encode_bytes(&vec![0x61; cap], ecc).unwrap();
            assert_eq!(c.version(), v, "{ecc:?} at {cap} bytes");
            if v < 40 {
                let c = encode_bytes(&vec![0x61; cap + 1], ecc).unwrap();
                assert_eq!(c.version(), v + 1, "{ecc:?} at {} bytes", cap + 1);
            }
        }
    }
}

#[test]
fn qr_capacity_table_every_version_and_level() {
    for ecc in Ecc::ALL {
        for v in 1..=40u8 {
            let cc = Mode::Byte.count_bits(v) as usize;
            let max = (tables::data_codewords(v, ecc) * 8 - 4 - cc) / 8;
            let at = encode_bytes(&vec![0u8; max], ecc).unwrap();
            assert_eq!(at.version(), v, "{ecc:?} v{v} max {max}");
            match encode_bytes(&vec![0u8; max + 1], ecc) {
                Ok(c) => assert_eq!(c.version(), v + 1, "{ecc:?} v{v}"),
                Err(e) => {
                    assert_eq!(v, 40);
                    assert!(matches!(e, QrError::TooLong { .. }));
                }
            }
        }
    }
}

#[test]
fn qr_total_codewords_per_version() {
    // Total codewords per version (ISO/IEC 18004 Table 1), a sample.
    for (v, total) in [
        (1u8, 26usize),
        (2, 44),
        (7, 196),
        (14, 581),
        (21, 1156),
        (31, 2323),
        (32, 2465),
        (40, 3706),
    ] {
        assert_eq!(tables::total_codewords(v), total, "version {v}");
    }
}

#[test]
fn qr_too_long_is_refused_with_its_numbers() {
    let err = encode_bytes(&vec![0u8; 2954], Ecc::L).unwrap_err();
    assert_eq!(
        err,
        QrError::TooLong {
            needed_bits: 4 + 16 + 2954 * 8,
            max_bits: 2956 * 8
        }
    );
    assert!(err.to_string().starts_with("QR-01"));
    // A huge input is refused on its length alone.
    let big = vec![b'1'; 1 << 20];
    assert!(matches!(
        encode_text(std::str::from_utf8(&big).unwrap(), Ecc::L),
        Err(QrError::TooLong { .. })
    ));
    // Numeric capacity at 40-L is 7089 digits.
    assert_eq!(
        encode_text(&"7".repeat(7089), Ecc::L).unwrap().version(),
        40
    );
    assert!(encode_text(&"7".repeat(7090), Ecc::L).is_err());
}

#[test]
fn qr_forced_values_out_of_range() {
    assert_eq!(
        encode_with(b"1", Ecc::L, Some(0), None).unwrap_err(),
        QrError::BadVersion
    );
    assert_eq!(
        encode_with(b"1", Ecc::L, Some(41), None).unwrap_err(),
        QrError::BadVersion
    );
    assert_eq!(
        encode_with(b"1", Ecc::L, None, Some(8)).unwrap_err(),
        QrError::BadMask
    );
    assert!(QrError::BadVersion.to_string().starts_with("QR-02"));
    assert!(QrError::BadMask.to_string().starts_with("QR-03"));
    assert!(QrError::BadData.to_string().starts_with("QR-04"));
    // A forced version too small for the data is QR-01.
    assert!(matches!(
        encode_with(&[b'a'; 40], Ecc::L, Some(1), None),
        Err(QrError::TooLong { .. })
    ));
    // A mode that cannot carry the data.
    assert_eq!(
        encode_segment(Mode::Numeric, b"12a", Ecc::L, None, None).unwrap_err(),
        QrError::BadData
    );
    assert_eq!(
        encode_segment(Mode::Alphanumeric, b"abc", Ecc::L, None, None).unwrap_err(),
        QrError::BadData
    );
}

#[test]
fn qr_mode_choice() {
    assert_eq!(encode_text("0123", Ecc::L).unwrap().mode(), Mode::Numeric);
    assert_eq!(
        encode_text("A1 $%*+-./:", Ecc::L).unwrap().mode(),
        Mode::Alphanumeric
    );
    assert_eq!(encode_text("a", Ecc::L).unwrap().mode(), Mode::Byte);
    assert_eq!(encode_text("é", Ecc::L).unwrap().mode(), Mode::Byte);
    assert_eq!(encode_bytes(b"0123", Ecc::L).unwrap().mode(), Mode::Byte);
}

#[test]
fn qr_get_outside_is_light() {
    let c = encode_text("1", Ecc::L).unwrap();
    assert!(c.get(0, 0));
    assert!(!c.get(21, 0));
    assert!(!c.get(0, 21));
    assert!(!c.get(usize::MAX, usize::MAX));
}

#[test]
fn qr_equal_penalty_goes_to_the_lower_mask() {
    // The chosen mask is the first of the lowest penalties.
    for text in ["HELLO WORLD", "01234567", "bitcoin"] {
        let c = encode_text(text, Ecc::M).unwrap();
        let mut best = (u32::MAX, 0u8);
        for m in 0..8u8 {
            let forced = encode_with(text.as_bytes(), Ecc::M, Some(c.version()), Some(m)).unwrap();
            let p = penalty(forced.size, &forced.modules);
            if p < best.0 {
                best = (p, m);
            }
        }
        assert_eq!(c.mask(), best.1, "{text}");
    }
}

#[test]
fn qr_penalty_rule_1_runs() {
    assert_eq!(run_penalty(&bits("0000")), 0);
    assert_eq!(run_penalty(&bits("00000")), 3);
    assert_eq!(run_penalty(&bits("000000")), 4);
    assert_eq!(run_penalty(&bits("0000000")), 5);
    assert_eq!(run_penalty(&bits("11111000001")), 6);
    assert_eq!(run_penalty(&bits("01010101")), 0);
}

#[test]
fn qr_penalty_rule_2_blocks() {
    // 3 by 3, all light: four blocks.
    assert_eq!(block_penalty(3, &[false; 9]), 12);
    // A checkerboard has none.
    assert_eq!(block_penalty(3, &bits("101010101")), 0);
    // One light pair of rows over one dark: only the 2 by 2 of one colour.
    assert_eq!(block_penalty(3, &bits("110110001")), 3);
}

#[test]
fn qr_penalty_rule_3_finder_like() {
    let p = bits("10111010000");
    assert_eq!(finder_like_penalty(&p), 40);
    assert_eq!(finder_like_penalty(&bits("00001011101")), 40);
    // Light on both sides: counted once, not twice.
    assert_eq!(finder_like_penalty(&bits("000010111010000")), 40);
    // Two patterns.
    assert_eq!(finder_like_penalty(&bits("1011101000010111010000")), 80);
    // At the edge with fewer than four light modules inside the symbol.
    assert_eq!(finder_like_penalty(&bits("1011101")), 0);
    assert_eq!(finder_like_penalty(&bits("1011101000")), 0);
    assert_eq!(finder_like_penalty(&bits("0001011101")), 0);
    // Dark next to it, not light.
    assert_eq!(finder_like_penalty(&bits("010111010000")), 40);
    assert_eq!(finder_like_penalty(&bits("11011101")), 0);
}

#[test]
fn qr_penalty_rule_4_balance() {
    assert_eq!(balance_penalty(50, 100), 0);
    assert_eq!(balance_penalty(54, 100), 0);
    assert_eq!(balance_penalty(45, 100), 0);
    assert_eq!(balance_penalty(49, 100), 0);
    assert_eq!(balance_penalty(55, 100), 10);
    assert_eq!(balance_penalty(59, 100), 10);
    assert_eq!(balance_penalty(44, 100), 10);
    assert_eq!(balance_penalty(60, 100), 20);
    assert_eq!(balance_penalty(65, 100), 30);
    assert_eq!(balance_penalty(0, 100), 90);
    assert_eq!(balance_penalty(100, 100), 100);
}

#[test]
fn qr_penalty_whole_matrix() {
    // 5 by 5 all light: ten lines of one run of 5 (3 each), sixteen 2 by 2
    // blocks (3 each), and a balance of 0 percent dark (90).
    assert_eq!(penalty(5, &[false; 25]), 30 + 48 + 90);
    // 5 by 5 checkerboard: 13 dark of 25 is 52 percent, nothing else.
    let board: Vec<bool> = (0..25).map(|i| i % 2 == 0).collect();
    assert_eq!(penalty(5, &board), 0);
}

#[test]
fn qr_bitcoin_uri_modes() {
    let addr = "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3";
    assert_eq!(addr.len(), 62);
    // A bare upper-case URI is alphanumeric: 4 + 9 + 35 * 11 = 398 bits.
    let bare = format!("BITCOIN:{}", addr.to_uppercase());
    assert_eq!(bare.len(), 70);
    for (ecc, version) in [(Ecc::L, 3), (Ecc::M, 4), (Ecc::Q, 5), (Ecc::H, 6)] {
        let c = encode_text(&bare, ecc).unwrap();
        assert_eq!(c.mode(), Mode::Alphanumeric, "{ecc:?}");
        assert_eq!(c.version(), version, "{ecc:?}");
    }
    // `?` and `=` are not in the 45-character set, so a URI with an amount
    // is byte mode: 4 + 8 + 88 * 8 = 716 bits.
    let uri = format!("{bare}?AMOUNT=0.00012345");
    assert_eq!(uri.len(), 88);
    for (ecc, version) in [(Ecc::L, 5), (Ecc::M, 6), (Ecc::Q, 8), (Ecc::H, 9)] {
        let c = encode_text(&uri, ecc).unwrap();
        assert_eq!(c.mode(), Mode::Byte, "{ecc:?}");
        assert_eq!(c.version(), version, "{ecc:?}");
    }
}

#[test]
fn qr_generator_polynomial_degree_7() {
    // x^7 + a^87 x^6 + a^229 x^5 + a^146 x^4 + a^149 x^3 + a^238 x^2
    // + a^102 x + a^21, with a^k as its value in GF(256), ISO/IEC 18004
    // Annex A.
    let want = [127u8, 122, 154, 164, 11, 68, 117];
    assert_eq!(crate::rs::divisor(7), want);
}

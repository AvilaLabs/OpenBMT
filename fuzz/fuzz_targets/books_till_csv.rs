#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_books::{read_till_csv, Books, TillMapping};

fuzz_target!(|data: &[u8]| {
    // The till's CSV reader (BK-46): the first four bytes choose the columns,
    // the header flag and the offset from UTC, the rest is the file. It must
    // never panic; a file that reads names only lines that exist, and the
    // match of its lines against empty books (BK-47) must never panic and
    // must find nothing paid for any line.
    let Some((head, file)) = data.split_first_chunk::<4>() else {
        return;
    };
    let mapping = TillMapping {
        reference: head[0] % 6,
        amount: head[1] % 6,
        payment_type: head[2] % 6,
        time: (head[3] >> 1) % 6,
        bitcoin_type: "Bitcoin".to_string(),
        has_header: head[3] & 1 == 1,
    };
    let offset = i32::from(head[0] >> 4) * 60 - 600;
    match read_till_csv(file, &mapping, offset) {
        Ok(lines) => {
            // Lines end at `\n`, `\r\n` or a lone `\r`.
            let rows = file
                .iter()
                .enumerate()
                .filter(|(i, b)| **b == b'\n' || (**b == b'\r' && file.get(i + 1) != Some(&b'\n')))
                .count()
                + 1;
            for line in &lines {
                assert!(line.line >= 1 && line.line <= rows);
            }
            let matched = Books::default().match_till(&lines, &mapping, offset);
            assert!(matched.matched.is_empty());
        }
        Err(e) => {
            assert!(!e.rule().is_empty());
            let _ = e.words();
        }
    }
});

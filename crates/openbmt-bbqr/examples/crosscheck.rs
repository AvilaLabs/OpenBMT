//! The OpenBMT side of the cross-check against Coinkite's reference
//! implementation (`scripts/bbqr-crosscheck.sh`).
//!
//!   crosscheck read CASES            decode the parts the reference wrote
//!   crosscheck write CASES OURS      write this library's parts for the same data
//!
//! The files hold `case <name> <kind> <hex of the data>`, then for each set
//! `parts <label>` and one part per line (see `crosscheck/bbqr_cross.py`).
//! `read` joins each set of the reference, in order and reversed, and the
//! bytes and the type must be the case's. `write` writes, for each case,
//! the parts of this library at several part sizes.

use openbmt_bbqr::{encode, Joiner, Kind};

const ALL: &[Kind] = &[
    Kind::Psbt,
    Kind::Transaction,
    Kind::Json,
    Kind::Cbor,
    Kind::Text,
    Kind::Binary,
    Kind::Executable,
];

struct Case {
    name: String,
    kind: char,
    data: Vec<u8>,
    sets: Vec<(String, Vec<String>)>,
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .filter_map(|i| u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn read_cases(path: &str) -> Vec<Case> {
    let text = std::fs::read_to_string(path).expect("read the file");
    let mut cases: Vec<Case> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("case ") {
            let f: Vec<&str> = rest.split(' ').collect();
            cases.push(Case {
                name: f[0].to_string(),
                kind: f[1].chars().next().expect("kind"),
                data: unhex(f[2]),
                sets: Vec::new(),
            });
        } else if let Some(label) = line.strip_prefix("parts ") {
            cases
                .last_mut()
                .expect("a case")
                .sets
                .push((label.to_string(), Vec::new()));
        } else if !line.is_empty() {
            cases
                .last_mut()
                .expect("a case")
                .sets
                .last_mut()
                .expect("a set")
                .1
                .push(line.to_string());
        }
    }
    cases
}

fn join(parts: &[String]) -> (Kind, Vec<u8>) {
    let mut j = Joiner::new(ALL);
    for p in parts {
        j.add(p).expect("a part");
    }
    let d = j.finish().expect("a file");
    (d.kind, d.bytes)
}

fn read(path: &str) {
    let mut sets = 0;
    for c in read_cases(path) {
        let kind = Kind::from_code(c.kind).expect("kind");
        for (label, parts) in &c.sets {
            let (k, bytes) = join(parts);
            assert_eq!(k, kind, "{} {label}: the type", c.name);
            assert_eq!(bytes, c.data, "{} {label}: the bytes", c.name);
            let mut back = parts.clone();
            back.reverse();
            assert_eq!(join(&back).1, c.data, "{} {label}: reversed", c.name);
            sets += 1;
        }
    }
    println!("PASS  openbmt-bbqr read {sets} sets written by the reference");
}

fn write(cases: &str, ours: &str) {
    let mut out = String::new();
    let mut sets = 0;
    for c in read_cases(cases) {
        let kind = Kind::from_code(c.kind).expect("kind");
        out.push_str(&format!("case {} {} {}\n", c.name, c.kind, hex(&c.data)));
        for chars in [100usize, 400, 1000, 2132, 4296] {
            let Ok(parts) = encode(kind, &c.data, chars) else {
                continue;
            };
            out.push_str(&format!("parts ours-2-{chars}\n"));
            for p in parts {
                out.push_str(&p);
                out.push('\n');
            }
            sets += 1;
        }
    }
    std::fs::write(ours, out).expect("write the file");
    println!("wrote {sets} sets");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[1..] {
        ["read", cases] => read(cases),
        ["write", cases, ours] => write(cases, ours),
        _ => {
            eprintln!("usage: crosscheck read CASES | crosscheck write CASES OURS");
            std::process::exit(2);
        }
    }
}

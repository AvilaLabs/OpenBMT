//! WO-16, WO-50: the module matrices the web order plugin's PHP encoder
//! (`integrations/woocommerce/openbmt-pay/src/Qr.php`) must make exactly: texts
//! of every mode at every level across the version boundaries, and the
//! payment links of the plugin from the shortest to the longest.
//! `OPENBMT_BLESS_WEB_ORDER=1` writes the file again.

use std::fmt::Write as _;
use std::path::PathBuf;

use openbmt_qr::{encode_text, Ecc, Mode};

fn file_path() -> PathBuf {
    let root = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_default();
    root.join("../../integrations/woocommerce/openbmt-pay/tests_standalone/vectors/qr.json")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn q(text: &str) -> String {
    format!("\"{text}\"")
}

fn level(ecc: Ecc) -> &'static str {
    match ecc {
        Ecc::L => "L",
        Ecc::M => "M",
        Ecc::Q => "Q",
        Ecc::H => "H",
    }
}

fn mode(m: Mode) -> &'static str {
    match m {
        Mode::Numeric => "numeric",
        Mode::Alphanumeric => "alphanumeric",
        Mode::Byte => "byte",
    }
}

/// One case: the text as hex and what the encoder makes of it, the rows of
/// the matrix as hex, most significant bit first, a row to a string.
fn case(text: &str, ecc: Ecc) -> String {
    let code = encode_text(text, ecc).expect("fits");
    let size = code.size();
    let rows: Vec<String> = (0..size)
        .map(|y| {
            let mut bytes = vec![0u8; size.div_ceil(8)];
            for x in 0..size {
                if code.get(x, y) {
                    if let Some(b) = bytes.get_mut(x / 8) {
                        *b |= 0x80 >> (x % 8);
                    }
                }
            }
            q(&hex(&bytes))
        })
        .collect();
    format!(
        "  {{\"text\":{},\"ecc\":{},\"version\":{},\"mask\":{},\"mode\":{},\"size\":{size},\"rows\":[{}]}}",
        q(&hex(text.as_bytes())),
        q(level(ecc)),
        code.version(),
        code.mask(),
        q(mode(code.mode())),
        rows.join(",")
    )
}

/// Percent-encoding of everything but the unreserved characters of RFC 3986.
fn rawurlencode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

fn btc_text(sats: u64) -> String {
    let whole = sats / 100_000_000;
    let frac = format!("{:08}", sats % 100_000_000);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{frac}")
    }
}

fn link(address: &str, sats: u64, label: &str) -> String {
    let mut query = Vec::new();
    if sats > 0 {
        query.push(format!("amount={}", btc_text(sats)));
    }
    if !label.is_empty() {
        query.push(format!("label={}", rawurlencode(label)));
    }
    if query.is_empty() {
        format!("bitcoin:{address}")
    } else {
        format!("bitcoin:{address}?{}", query.join("&"))
    }
}

fn qr_text(link: &str) -> String {
    match link.split_once('?') {
        Some((head, tail)) => format!("{}?{tail}", head.to_uppercase()),
        None => link.to_uppercase(),
    }
}

fn file() -> String {
    let mut texts: Vec<String> = vec![
        String::new(),
        "0".into(),
        "01234567".into(),
        "0".repeat(200),
        "HELLO WORLD".into(),
        "BITCOIN:BC1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KV8F3T4".into(),
        "hello, world".into(),
        "Caf\u{e9} \u{20ac}".into(),
    ];
    for len in [
        1usize, 17, 18, 32, 33, 53, 54, 78, 79, 106, 107, 122, 123, 150, 151, 300, 500, 1000,
    ] {
        texts.push("a".repeat(len));
    }
    let mut cases = Vec::new();
    for t in &texts {
        for ecc in Ecc::ALL {
            cases.push(case(t, ecc));
        }
    }
    let taproot = "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0";
    let longest = format!("bc1p{}", "q".repeat(86));
    let long_label = "\u{20ac}".repeat(21);
    let links: Vec<(&str, u64, String)> = vec![
        (
            "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4",
            100_000,
            "Shop".into(),
        ),
        (
            "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3",
            1,
            "Caf\u{e9} & Co".into(),
        ),
        (taproot, 2_100_000_000_000_000, long_label.clone()),
        (&longest, 2_100_000_000_000_000, long_label),
        (
            "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4",
            0,
            String::new(),
        ),
        (taproot, 12_345_678, "A".into()),
    ];
    let mut link_rows = Vec::new();
    for (address, sats, label) in &links {
        let l = link(address, *sats, label);
        let t = qr_text(&l);
        for ecc in Ecc::ALL {
            cases.push(case(&t, ecc));
        }
        link_rows.push(format!(
            "  {{\"address\":{},\"sats\":{sats},\"label\":{},\"link\":{},\"qr_text\":{}}}",
            q(address),
            q(&hex(label.as_bytes())),
            q(&l),
            q(&t)
        ));
    }
    format!(
        "{{\n\"version\":1,\n\"links\":[\n{}\n],\n\"codes\":[\n{}\n]\n}}\n",
        link_rows.join(",\n"),
        cases.join(",\n")
    )
}

#[test]
fn wo_16_the_committed_qr_vectors_are_the_ones_the_encoder_makes() {
    let text = file();
    let path = file_path();
    if std::env::var_os("OPENBMT_BLESS_WEB_ORDER").is_some() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a directory");
        }
        std::fs::write(&path, &text).expect("written");
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == text,
        "the vector file differs from the encoder: run the tests of this crate with \
         OPENBMT_BLESS_WEB_ORDER=1 to write it again"
    );
}

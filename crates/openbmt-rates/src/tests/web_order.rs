//! WO-15, WO-50: the vectors the web order plugin shares with these rules.
//! The plugin's quote is checked against the answers of this library; the
//! inputs are those of the tests of RT-20 to RT-38 and of CR-08. With
//! `OPENBMT_BLESS_WEB_ORDER=1` the test writes the file again.

use std::path::PathBuf;

use openbmt_core::currency::ShopCurrency;
use openbmt_counter::money::{sats_for, Amount, Rate};

use crate::{combine, parse_price_minor, parse_response, source, Sample, SOURCES};
use crate::{ParseRefused, PriceRefused, RatesRefused};

const NOW: u64 = 1_800_000_000;

fn path() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it when it runs tests"))
        .join("../../integrations/woocommerce/openbmt-pay/tests_standalone/vectors/rates.json")
}

fn q(text: &str) -> String {
    format!("\"{text}\"")
}

fn price_name(e: PriceRefused) -> &'static str {
    match e {
        PriceRefused::Empty => "Empty",
        PriceRefused::Sign => "Sign",
        PriceRefused::Exponent => "Exponent",
        PriceRefused::BadCharacter => "BadCharacter",
        PriceRefused::TwoDots => "TwoDots",
        PriceRefused::BadShape => "BadShape",
        PriceRefused::LeadingZero => "LeadingZero",
        PriceRefused::TooManyDecimals => "TooManyDecimals",
        PriceRefused::Zero => "Zero",
        PriceRefused::OutOfBand => "OutOfBand",
    }
}

fn table() -> String {
    let rows: Vec<String> = SOURCES
        .iter()
        .map(|s| {
            format!(
                "  {{\"id\":{},\"initial\":{},\"currency\":{},\"url\":{}}}",
                q(s.id),
                q(&s.initial.to_string()),
                q(s.currency.code()),
                q(s.url)
            )
        })
        .collect();
    rows.join(",\n")
}

fn prices() -> String {
    let long = "9".repeat(200);
    let long_fraction = format!("{long}.5");
    let texts: Vec<(&str, String)> = [
        ("USD", "73550.01"),
        ("USD", "73550"),
        ("USD", "73550.1"),
        ("USD", "73550.10000000"),
        ("USD", "100"),
        ("USD", "100000000"),
        ("USD", ""),
        ("USD", "+73550.01"),
        ("USD", "-73550.01"),
        ("USD", "7.355e4"),
        ("USD", "7.355E4"),
        ("USD", "1e5"),
        ("USD", "73550.01.1"),
        ("USD", ".."),
        ("USD", "."),
        ("USD", ".5"),
        ("USD", "73550."),
        ("USD", "073550.01"),
        ("USD", "00"),
        ("USD", "73,550.01"),
        ("USD", "73550.01 "),
        ("USD", " 73550.01"),
        ("USD", "$73550"),
        ("USD", "0x10"),
        ("USD", "NaN"),
        ("USD", "inf"),
        ("USD", "7355\u{0660}"),
        ("USD", "73\u{ff15}50"),
        ("USD", "73550.123456789"),
        ("USD", "73550.000000000"),
        ("USD", "73550.12345678"),
        ("USD", "100.004"),
        ("USD", "100.005"),
        ("USD", "100.0049999"),
        ("USD", "100.00499999"),
        ("USD", "100.00500000"),
        ("USD", "73552.175"),
        ("USD", "73552.174"),
        ("USD", "73552.9951"),
        ("USD", "0"),
        ("USD", "0.0"),
        ("USD", "0.004"),
        ("USD", "0.00499999"),
        ("USD", "99.99"),
        ("USD", "99.994"),
        ("USD", "99.995"),
        ("USD", "1"),
        ("USD", "100000000.00"),
        ("USD", "100000000.01"),
        ("USD", "100000001"),
        ("JPY", "11868632.4"),
        ("JPY", "11868632.5"),
        ("KRW", "100674000"),
        ("IDR", "1234567890.125"),
        ("INR", "7442024.5"),
        ("JPY", "100"),
        ("JPY", "1000000"),
        ("JPY", "999999"),
        ("JPY", "1000000000000"),
        ("JPY", "1000000000001"),
        ("IDR", "1000000"),
        ("IDR", "999999.99"),
        ("SAT", "73550"),
    ]
    .iter()
    .map(|(c, t)| (*c, (*t).to_string()))
    .chain([("USD", long), ("USD", long_fraction)])
    .collect();
    let rows: Vec<String> = texts
        .iter()
        .map(|(code, text)| {
            let currency = ShopCurrency::from_code(code).expect("a currency");
            let want = match parse_price_minor(text, currency) {
                Ok(minor) => format!("{{\"minor\":{minor}}}"),
                Err(e) => format!("{{\"refused\":{}}}", q(price_name(e))),
            };
            let escaped: String = text
                .chars()
                .map(|c| match c {
                    ' '..='~' => c.to_string(),
                    _ => format!("\\u{:04x}", u32::from(c)),
                })
                .collect();
            format!(
                "  {{\"currency\":{},\"text\":\"{escaped}\",\"expect\":{want}}}",
                q(code)
            )
        })
        .collect();
    rows.join(",\n")
}

type Case = (&'static str, Vec<(&'static str, u64, u64)>);

/// Each case is the time and the samples as (source, age in seconds, price).
/// An age above `NOW` is a time ahead: it is given as `NOW + ahead`.
fn combine_cases() -> Vec<Case> {
    let a = |id, age, price| (id, NOW - age, price);
    vec![
        (
            "two agree",
            vec![a("kraken", 30, 7_355_450), a("coinbase", 10, 7_355_218)],
        ),
        (
            "three give the middle",
            vec![
                a("gemini", 5, 7_356_540),
                a("kraken", 40, 7_355_450),
                a("bitstamp", 20, 7_355_001),
            ],
        ),
        (
            "four give the lower middle",
            vec![
                a("kraken", 1, 7_000_300),
                a("coinbase", 1, 7_000_100),
                a("bitstamp", 1, 7_000_400),
                a("gemini", 1, 7_000_200),
            ],
        ),
        (
            "stale leaves one",
            vec![a("kraken", 601, 7_355_450), a("coinbase", 10, 7_355_218)],
        ),
        (
            "age limit",
            vec![a("kraken", 600, 7_355_450), a("coinbase", 0, 7_355_218)],
        ),
        (
            "ahead limit",
            vec![("kraken", NOW + 60, 7_355_450), a("coinbase", 0, 7_355_218)],
        ),
        (
            "too far ahead",
            vec![("kraken", NOW + 61, 7_355_450), a("coinbase", 0, 7_355_218)],
        ),
        (
            "stale does not count in the spread",
            vec![
                a("gemini", 601, 1_000_000),
                a("kraken", 1, 7_355_450),
                a("coinbase", 1, 7_355_218),
            ],
        ),
        ("one source", vec![a("kraken", 1, 7_355_450)]),
        ("none", vec![]),
        (
            "one source twice",
            vec![a("kraken", 1, 7_355_450), a("kraken", 2, 7_355_450)],
        ),
        (
            "newest of a source",
            vec![
                a("kraken", 50, 7_000_000),
                a("kraken", 5, 7_355_450),
                a("coinbase", 10, 7_355_218),
            ],
        ),
        (
            "spread at the limit",
            vec![a("kraken", 1, 10_000_000), a("coinbase", 1, 10_200_000)],
        ),
        (
            "spread over the limit",
            vec![a("kraken", 1, 10_000_000), a("coinbase", 1, 10_200_001)],
        ),
        (
            "below the median over the limit",
            vec![
                a("kraken", 1, 9_799_999),
                a("coinbase", 1, 10_000_000),
                a("gemini", 1, 10_000_000),
            ],
        ),
        (
            "below the median at the limit",
            vec![
                a("kraken", 1, 9_800_000),
                a("coinbase", 1, 10_000_000),
                a("gemini", 1, 10_000_000),
            ],
        ),
        (
            "an outlier is not dropped",
            vec![
                a("kraken", 1, 7_355_450),
                a("coinbase", 1, 7_355_218),
                a("bitstamp", 1, 9_000_000),
            ],
        ),
        (
            "all stale",
            vec![a("kraken", 601, 7_355_450), a("coinbase", 660, 7_355_218)],
        ),
        (
            "oldest time",
            vec![
                a("kraken", 100, 7_355_450),
                a("coinbase", 500, 7_355_218),
                a("gemini", 3, 7_356_540),
            ],
        ),
        (
            "mixed currencies",
            vec![a("coinbase", 1, 7_355_218), a("kraken-eur", 1, 7_355_450)],
        ),
        (
            "yen",
            vec![
                a("kraken-jpy", 5, 11_868_000),
                a("bitflyer-jpy", 9, 11_870_000),
            ],
        ),
        (
            "sample under the band",
            vec![a("kraken", 1, 9_999), a("coinbase", 1, 7_355_218)],
        ),
        (
            "sample over the band",
            vec![a("kraken", 1, 10_000_000_001), a("coinbase", 1, 7_355_218)],
        ),
        (
            "sample at the band",
            vec![a("kraken", 1, 10_000), a("coinbase", 1, 10_000)],
        ),
    ]
}

fn combines() -> String {
    let rows: Vec<String> = combine_cases()
        .iter()
        .map(|(name, samples)| {
            let made: Result<Vec<Sample>, PriceRefused> = samples
                .iter()
                .map(|(id, at, price)| Sample::new(source(id).expect("a source"), *at, *price))
                .collect();
            let want = match made {
                Err(e) => format!("{{\"refused\":{}}}", q(price_name(e))),
                Ok(list) => match combine(&list, NOW) {
                    Ok((rate, from)) => format!(
                        "{{\"rate\":{},\"label\":{},\"oldest\":{}}}",
                        rate.minor_per_btc(),
                        q(from.label()),
                        from.fetched_at()
                    ),
                    Err(RatesRefused::TooOld) => "{\"refused\":\"TooOld\"}".to_string(),
                    Err(RatesRefused::NotEnoughSources { fresh }) => {
                        format!("{{\"refused\":\"NotEnoughSources\",\"fresh\":{fresh}}}")
                    }
                    Err(RatesRefused::PricesDisagree { lowest, highest }) => format!(
                        "{{\"refused\":\"PricesDisagree\",\"lowest\":{lowest},\"highest\":{highest}}}"
                    ),
                    Err(RatesRefused::MixedCurrencies) => {
                        "{\"refused\":\"MixedCurrencies\"}".to_string()
                    }
                },
            };
            let list: Vec<String> = samples
                .iter()
                .map(|(id, at, price)| {
                    format!("{{\"id\":{},\"fetched_at\":{at},\"price\":{price}}}", q(id))
                })
                .collect();
            format!(
                "  {{\"name\":{},\"now\":{NOW},\"samples\":[{}],\"expect\":{want}}}",
                q(name),
                list.join(",")
            )
        })
        .collect();
    rows.join(",\n")
}

fn sats() -> String {
    let cases: [(u64, u64); 12] = [
        (2_500, 7_355_218),
        (1, 7_355_218),
        (10_000, 10_000),
        (7_355_218, 7_355_218),
        (7_355_219, 7_355_218),
        (100_000_000, 7_355_218),
        (100, 10_000_000_000),
        (1, 10_000_000_000),
        (123_456_789, 1_234_567),
        (10_000_000_000, 10_000),
        (99_999_999_999, 10_000),
        (1_000_000_000_000_000, 10_000),
    ];
    let rows: Vec<String> = cases
        .iter()
        .map(|(amount, rate)| {
            let want = sats_for(Amount(*amount), Rate::new(*rate).expect("a rate"))
                .map_or("null".to_string(), |s| s.to_string());
            format!("  {{\"amount\":{amount},\"rate\":{rate},\"sats\":{want}}}")
        })
        .collect();
    rows.join(",\n")
}

/// One letter for what the reader made of a reply: `+` for a price, a capital
/// for a refusal of the reply, a lower case letter for one of the price text.
fn code(result: &Result<u64, ParseRefused>) -> char {
    match result {
        Ok(_) => '+',
        Err(ParseRefused::TooLong) => 'L',
        Err(ParseRefused::TooDeep) => 'D',
        Err(ParseRefused::NotUtf8) => 'U',
        Err(ParseRefused::Malformed) => 'M',
        Err(ParseRefused::TrailingBytes) => 'T',
        Err(ParseRefused::DuplicateKey) => 'K',
        Err(ParseRefused::MissingField) => 'F',
        Err(ParseRefused::WrongShape) => 'S',
        Err(ParseRefused::WrongCurrency) => 'C',
        Err(ParseRefused::SourceError) => 'E',
        Err(ParseRefused::Price(p)) => match p {
            PriceRefused::Empty => 'a',
            PriceRefused::Sign => 'b',
            PriceRefused::Exponent => 'c',
            PriceRefused::BadCharacter => 'd',
            PriceRefused::TwoDots => 'e',
            PriceRefused::BadShape => 'f',
            PriceRefused::LeadingZero => 'g',
            PriceRefused::TooManyDecimals => 'h',
            PriceRefused::Zero => 'i',
            PriceRefused::OutOfBand => 'j',
        },
    }
}

fn fixtures_folder() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it when it runs tests"))
        .join("tests/fixtures")
}

/// The real and documented replies of the fixtures, by the name of their
/// source, in the order of the table.
fn fixtures() -> Vec<(String, Vec<u8>)> {
    SOURCES
        .iter()
        .filter_map(|s| {
            std::fs::read(fixtures_folder().join(format!("{}.json", s.id)))
                .ok()
                .map(|b| (s.id.to_string(), b))
        })
        .collect()
}

fn replace_first(body: &[u8], from: &[u8], to: &[u8]) -> Option<Vec<u8>> {
    let at = body.windows(from.len()).position(|w| w == from)?;
    let mut out = body.get(..at)?.to_vec();
    out.extend_from_slice(to);
    out.extend_from_slice(body.get(at + from.len()..)?);
    Some(out)
}

/// The replies the reader is tried on, each as a name, its bytes and the
/// number of spaces added after them.
fn replies() -> Vec<(String, Vec<u8>, usize)> {
    let mut out: Vec<(String, Vec<u8>, usize)> = Vec::new();
    let fixed = fixtures();
    for (id, body) in &fixed {
        out.push((format!("fixture {id}"), body.clone(), 0));
    }
    for (id, body) in &fixed {
        let mut dup = b"{\"zz\":1,\"zz\":2,".to_vec();
        dup.extend_from_slice(body.get(1..).unwrap_or(&[]));
        if body.first() == Some(&b'{') {
            out.push((format!("duplicate key before {id}"), dup, 0));
        }
        let mut trailing = body.clone();
        trailing.extend_from_slice(b" x");
        out.push((format!("trailing bytes after {id}"), trailing, 0));
        let half = body.get(..body.len() / 2).unwrap_or(&[]).to_vec();
        out.push((format!("half of {id}"), half, 0));
        // A number shape with its price key spelled with an escape.
        if let Some(src) = source(id) {
            if let crate::Shape::Number { price, .. } = src.shape {
                if let Some(crate::Step::Key(key)) = price.last() {
                    let from = format!("\"{key}\"");
                    let first = key.chars().next().unwrap_or('a');
                    let to = format!("\"\\u{:04x}{}\"", u32::from(first), &key[1..]);
                    if let Some(b) = replace_first(body, from.as_bytes(), to.as_bytes()) {
                        out.push((format!("escaped price key in {id}"), b, 0));
                    }
                }
            }
        }
    }
    let plain: &[(&str, &[u8])] = &[
        ("empty", b""),
        ("open brace", b"{"),
        ("empty object", b"{}"),
        ("empty array", b"[]"),
        ("null", b"null"),
        ("half a literal", b"nul"),
        ("a string", b"\"x\""),
        ("a number", b"7"),
        ("NaN", b"{\"last\":NaN}"),
        ("Infinity", b"{\"last\":Infinity}"),
        ("leading zero", b"{\"ltp\":01}"),
        ("dot without digit", b"{\"ltp\":1.}"),
        ("dot first", b"{\"ltp\":.5}"),
        ("lone minus", b"{\"ltp\":-}"),
        ("exponent without digit", b"{\"ltp\":1e}"),
        ("single quotes", b"{'last':'1'}"),
        ("trailing comma", b"{\"last\":\"73550.1\",}"),
        ("missing colon", b"{\"last\" \"73550.1\"}"),
        ("raw control byte", b"{\"last\":\"73\x0050\"}"),
        ("bad escape", b"{\"last\":\"7\\x\"}"),
        ("lone low surrogate", b"{\"last\":\"\\udc00\"}"),
        ("lone high surrogate", b"{\"last\":\"\\ud800\"}"),
        ("short unicode escape", b"{\"last\":\"\\u12\"}"),
        ("good surrogate pair", b"{\"last\":\"\\ud83d\\ude00\"}"),
        ("price with an escape", b"{\"last\":\"7\\u0033550.1\"}"),
        ("invalid utf-8", b"{\"last\":\"73\xff\"}"),
        ("overlong utf-8", b"{\"last\":\"\xc0\xaf\"}"),
        ("surrogate in utf-8", b"{\"last\":\"\xed\xa0\x80\"}"),
        ("byte order mark", b"\xef\xbb\xbf{\"last\":\"73550.1\"}"),
        ("white space around", b" \t\r\n{\"last\":\"73550.1\"}\n "),
        ("form feed as space", b"\x0c{\"last\":\"73550.1\"}"),
        (
            "duplicate key",
            b"{\"last\":\"73550.1\",\"last\":\"73551.1\"}",
        ),
        (
            "duplicate key by escape",
            b"{\"last\":\"73550.1\",\"l\\u0061st\":\"1\"}",
        ),
        (
            "nested duplicate key",
            b"{\"last\":\"73550.1\",\"x\":{\"a\":1,\"a\":2}}",
        ),
        ("a number for a string", b"{\"last\":73550.1}"),
        ("a string for a number", b"{\"ltp\":\"73550.1\"}"),
        ("number price", b"{\"ltp\":11868632.5}"),
        ("number with exponent", b"{\"ltp\":1e5}"),
        ("negative number", b"{\"ltp\":-5}"),
        ("zero number", b"{\"ltp\":0}"),
        ("huge exponent", b"{\"ltp\":1.5e400}"),
        (
            "kraken with an error",
            b"{\"error\":[\"EQuery\"],\"result\":{\"X\":{\"c\":[\"73550.1\",\"1\"]}}}",
        ),
        (
            "kraken two entries",
            b"{\"error\":[],\"result\":{\"X\":{\"c\":[\"73550.1\"]},\"Y\":{\"c\":[\"1\"]}}}",
        ),
        (
            "kraken empty c",
            b"{\"error\":[],\"result\":{\"X\":{\"c\":[]}}}",
        ),
        (
            "kraken good",
            b"{\"error\":[],\"result\":{\"X\":{\"c\":[\"73550.1\",\"1\"]}}}",
        ),
        (
            "coinbase good",
            b"{\"data\":{\"amount\":\"73552.175\",\"currency\":\"USD\"}}",
        ),
        (
            "coinbase lower case currency",
            b"{\"data\":{\"amount\":\"73552.175\",\"currency\":\"usd\"}}",
        ),
        ("last good", b"{\"last\":\"73550.1\"}"),
        ("empty data array", b"{\"data\":[]}"),
        ("ndax number", b"{\"LastTradedPx\":9500000.5}"),
        ("ndax exponent", b"{\"LastTradedPx\":1e5}"),
        ("ndax negative", b"{\"LastTradedPx\":-5}"),
        ("ndax zero", b"{\"LastTradedPx\":0}"),
        ("ndax zero fraction", b"{\"LastTradedPx\":0.004}"),
        ("ndax huge exponent", b"{\"LastTradedPx\":1.5e400}"),
        ("ndax nine decimals", b"{\"LastTradedPx\":73550.123456789}"),
        ("ndax leading zero text", b"{\"LastTradedPx\":0.5}"),
        ("ndax null", b"{\"LastTradedPx\":null}"),
        ("ndax under the band", b"{\"LastTradedPx\":99.99}"),
    ];
    for (name, body) in plain {
        out.push(((*name).to_string(), body.to_vec(), 0));
    }
    let digits = "9".repeat(3000);
    out.push((
        "huge number in a string".into(),
        format!("{{\"last\":\"{digits}\"}}").into_bytes(),
        0,
    ));
    out.push((
        "huge fraction in a string".into(),
        format!("{{\"last\":\"{digits}.5\"}}").into_bytes(),
        0,
    ));
    out.push((
        "huge number".into(),
        format!("{{\"ltp\":{digits}}}").into_bytes(),
        0,
    ));
    out.push((
        "huge number with a fraction".into(),
        format!("{{\"ltp\":{digits}.5}}").into_bytes(),
        0,
    ));
    out.push((
        "too many decimals".into(),
        b"{\"last\":\"73550.123456789\"}".to_vec(),
        0,
    ));
    out.push((
        "nesting 16 deep".into(),
        format!("{}{}", "[".repeat(16), "]".repeat(16)).into_bytes(),
        0,
    ));
    out.push((
        "nesting 17 deep".into(),
        format!("{}{}", "[".repeat(17), "]".repeat(17)).into_bytes(),
        0,
    ));
    out.push((
        "nesting 16 deep beside a price".into(),
        format!(
            "{{\"x\":{}{},\"last\":\"73550.1\"}}",
            "[".repeat(15),
            "]".repeat(15)
        )
        .into_bytes(),
        0,
    ));
    // The size limit of RT-10: 65,536 bytes are read, one more is refused.
    out.push((
        "exactly 64 KiB".into(),
        b"{\"last\":\"73550.1\"}".to_vec(),
        65_536 - 18,
    ));
    out.push((
        "one byte over 64 KiB".into(),
        b"{\"last\":\"73550.1\"}".to_vec(),
        65_537 - 18,
    ));
    out
}

fn body_of(reply: &(String, Vec<u8>, usize)) -> Vec<u8> {
    let mut body = reply.1.clone();
    body.extend(std::iter::repeat_n(b' ', reply.2));
    body
}

fn reader() -> String {
    let all = replies();
    let rows: Vec<String> = all
        .iter()
        .map(|reply| {
            let body = body_of(reply);
            let mut codes = String::new();
            let mut minors = Vec::new();
            for s in &SOURCES {
                let r = parse_response(s, &body);
                codes.push(code(&r));
                if let Ok(minor) = r {
                    minors.push(format!("{}:{minor}", q(s.id)));
                }
            }
            format!(
                "  {{\"name\":{},\"body\":{},\"pad\":{},\"codes\":{},\"minors\":{{{}}}}}",
                q(&reply.0),
                q(&hex(&reply.1)),
                reply.2,
                q(&codes),
                minors.join(",")
            )
        })
        .collect();
    rows.join(",\n")
}

/// What the reader makes of every cut of each fixture, one letter for each
/// length from nothing to the whole reply.
fn truncations() -> String {
    let rows: Vec<String> = fixtures()
        .iter()
        .map(|(id, body)| {
            let src = source(id).expect("a source");
            let codes: String = (0..=body.len())
                .map(|n| code(&parse_response(src, body.get(..n).unwrap_or(&[]))))
                .collect();
            format!(
                "  {{\"source\":{},\"body\":{},\"codes\":{}}}",
                q(id),
                q(&hex(body)),
                q(&codes)
            )
        })
        .collect();
    rows.join(",\n")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn file() -> String {
    format!(
        "{{\n\"version\":2,\n\"table\":[\n{}\n],\n\"prices\":[\n{}\n],\n\"combine\":[\n{}\n],\n\"sats\":[\n{}\n],\n\"replies\":[\n{}\n],\n\"truncations\":[\n{}\n]\n}}\n",
        table(),
        prices(),
        combines(),
        sats(),
        reader(),
        truncations()
    )
}

#[test]
fn wo_15_the_committed_vectors_are_the_ones_the_library_makes() {
    let text = file();
    if std::env::var_os("OPENBMT_BLESS_WEB_ORDER").is_some() {
        std::fs::create_dir_all(path().parent().expect("a folder")).expect("a directory");
        std::fs::write(path(), &text).expect("written");
    }
    let committed = std::fs::read_to_string(path()).unwrap_or_default();
    assert!(
        committed == text,
        "the vector file differs from the library: run the tests of this crate with \
         OPENBMT_BLESS_WEB_ORDER=1 to write it again"
    );
}

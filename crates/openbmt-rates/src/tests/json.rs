//! RT-10 to RT-17: the reading of a reply.

use super::src;
use crate::json::{parse_document, Value, MAX_DEPTH, MAX_RESPONSE_BYTES};
use crate::{parse_response, ParseRefused, PriceRefused};

fn last(body: &str) -> Result<u64, ParseRefused> {
    parse_response(src("bitstamp"), body.as_bytes())
}

#[test]
fn rt_10_more_than_64_kib_is_refused_before_it_is_read() {
    // Not even JSON: the length decides.
    let big = vec![b'x'; MAX_RESPONSE_BYTES + 1];
    assert_eq!(
        parse_response(src("gemini"), &big),
        Err(ParseRefused::TooLong)
    );
    // Exactly the limit is read (and refused here for what it is).
    let at = vec![b' '; MAX_RESPONSE_BYTES];
    assert_eq!(
        parse_response(src("gemini"), &at),
        Err(ParseRefused::Malformed)
    );
    // A valid reply padded to the limit is read.
    let mut body = br#"{"last":"73550.01"}"#.to_vec();
    body.resize(MAX_RESPONSE_BYTES, b' ');
    assert_eq!(parse_response(src("bitstamp"), &body), Ok(7_355_001));
    body.push(b' ');
    assert_eq!(
        parse_response(src("bitstamp"), &body),
        Err(ParseRefused::TooLong)
    );
}

#[test]
fn rt_11_nesting_past_16_is_refused() {
    let nested = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert!(parse_document(nested(MAX_DEPTH).as_bytes()).is_ok());
    assert_eq!(
        parse_document(nested(MAX_DEPTH + 1).as_bytes()),
        Err(ParseRefused::TooDeep)
    );
    // Unclosed deep input is refused for its depth, not by running out of
    // stack.
    assert_eq!(
        parse_document("[".repeat(60_000).as_bytes()),
        Err(ParseRefused::TooDeep)
    );
    let objects = format!("{}1{}", r#"{"a":"#.repeat(17), "}".repeat(17));
    assert_eq!(
        parse_document(objects.as_bytes()),
        Err(ParseRefused::TooDeep)
    );
}

#[test]
fn rt_12_bytes_that_are_not_utf8_are_refused() {
    let mut body = br#"{"last":"73550.01","x":""#.to_vec();
    body.extend_from_slice(&[0xff, 0xfe]);
    body.extend_from_slice(br#""}"#);
    assert_eq!(
        parse_response(src("bitstamp"), &body),
        Err(ParseRefused::NotUtf8)
    );
    // An overlong form and a lone surrogate in UTF-8 are not UTF-8.
    assert_eq!(
        parse_document(&[b'"', 0xc0, 0x80, b'"']),
        Err(ParseRefused::NotUtf8)
    );
    assert_eq!(
        parse_document(&[b'"', 0xed, 0xa0, 0x80, b'"']),
        Err(ParseRefused::NotUtf8)
    );
}

#[test]
fn rt_13_bytes_after_the_document_are_refused() {
    assert_eq!(
        last(r#"{"last":"73550.01"}x"#),
        Err(ParseRefused::TrailingBytes)
    );
    assert_eq!(
        last(r#"{"last":"73550.01"}{}"#),
        Err(ParseRefused::TrailingBytes)
    );
    assert_eq!(
        last("{\"last\":\"73550.01\"}\0"),
        Err(ParseRefused::TrailingBytes)
    );
    // White space after is not "bytes after".
    assert_eq!(last("  {\"last\":\"73550.01\"} \r\n\t"), Ok(7_355_001));
}

#[test]
fn rt_13_a_document_that_is_not_json_is_refused() {
    for bad in [
        "",
        " ",
        "{",
        "{\"last\":}",
        "{\"last\" \"1\"}",
        "{last:\"1\"}",
        "{'last':'1'}",
        "{\"last\":\"1\",}",
        "[1,]",
        "[1 2]",
        "{\"a\":01}",
        "{\"a\":1.}",
        "{\"a\":.5}",
        "{\"a\":1e}",
        "{\"a\":-}",
        "{\"a\":tru}",
        "{\"a\":\"\\x\"}",
        "{\"a\":\"\\u12\"}",
        "{\"a\":\"\\ud800\"}",
        "{\"a\":\"\\udc00\"}",
        "{\"a\":\"\\ud800\\u0041\"}",
        "{\"a\":\"tab\there\"}",
        "{\"a\":\"open}",
        "NaN",
    ] {
        assert_eq!(
            parse_document(bad.as_bytes()),
            Err(ParseRefused::Malformed),
            "{bad:?}"
        );
    }
}

#[test]
fn rt_13_good_documents_with_every_kind_of_value_are_read() {
    let doc = r#" {"a":[1,-2.5e+3,0,true,false,null,"x\n\u00e9\ud83d\ude00"],"b":{},"c":[]} "#;
    let v = parse_document(doc.as_bytes()).expect("reads");
    let Some(Value::Arr(a)) = v.get("a") else {
        panic!("an array")
    };
    assert_eq!(a.len(), 7);
    assert_eq!(a.last().and_then(Value::as_str), Some("x\n\u{e9}\u{1f600}"));
}

#[test]
fn rt_14_a_key_twice_in_one_object_is_refused() {
    assert_eq!(
        last(r#"{"last":"73550.01","last":"1.00"}"#),
        Err(ParseRefused::DuplicateKey)
    );
    // However it is spelled.
    assert_eq!(
        last(r#"{"last":"73550.01","\u006cast":"1.00"}"#),
        Err(ParseRefused::DuplicateKey)
    );
    // Off the path too: a document that says things twice is not trusted.
    assert_eq!(
        last(r#"{"last":"73550.01","x":{"y":1,"y":2}}"#),
        Err(ParseRefused::DuplicateKey)
    );
    // The same key in two different objects is fine.
    assert_eq!(
        last(r#"{"last":"73550.01","x":{"last":1},"y":{"last":2}}"#),
        Ok(7_355_001)
    );
}

#[test]
fn rt_15_a_missing_or_mistyped_field_is_refused() {
    assert_eq!(last(r#"{}"#), Err(ParseRefused::MissingField));
    assert_eq!(
        last(r#"{"Last":"73550.01"}"#),
        Err(ParseRefused::MissingField)
    );
    // A number is not a price: prices are strings.
    assert_eq!(last(r#"{"last":73550.01}"#), Err(ParseRefused::WrongShape));
    assert_eq!(last(r#"{"last":null}"#), Err(ParseRefused::WrongShape));
    assert_eq!(
        last(r#"{"last":["73550.01"]}"#),
        Err(ParseRefused::WrongShape)
    );
    assert_eq!(last(r#"["73550.01"]"#), Err(ParseRefused::WrongShape));
    assert_eq!(last(r#""73550.01""#), Err(ParseRefused::WrongShape));
}

fn kraken(body: &str) -> Result<u64, ParseRefused> {
    parse_response(src("kraken"), body.as_bytes())
}

#[test]
fn rt_16_a_kraken_error_array_that_is_not_empty_is_a_refusal() {
    let ok = r#"{"error":[],"result":{"XXBTZUSD":{"c":["73554.50000","0.1"]}}}"#;
    assert_eq!(kraken(ok), Ok(7_355_450));
    assert_eq!(
        kraken(r#"{"error":["EQuery:Unknown asset pair"]}"#),
        Err(ParseRefused::SourceError)
    );
    // Even with a result beside it.
    assert_eq!(
        kraken(r#"{"error":["EGeneral:Temporary lockout"],"result":{"X":{"c":["73554.5","1"]}}}"#),
        Err(ParseRefused::SourceError)
    );
    assert_eq!(
        kraken(r#"{"error":"none","result":{}}"#),
        Err(ParseRefused::WrongShape)
    );
    assert_eq!(
        kraken(r#"{"result":{"X":{"c":["73554.5"]}}}"#),
        Err(ParseRefused::MissingField)
    );
}

#[test]
fn rt_15_kraken_result_holds_one_entry_with_a_last_trade_price() {
    assert_eq!(
        kraken(r#"{"error":[],"result":{}}"#),
        Err(ParseRefused::WrongShape)
    );
    assert_eq!(
        kraken(r#"{"error":[],"result":{"A":{"c":["73554.5"]},"B":{"c":["73554.5"]}}}"#),
        Err(ParseRefused::WrongShape)
    );
    assert_eq!(
        kraken(r#"{"error":[],"result":{"A":{}}}"#),
        Err(ParseRefused::MissingField)
    );
    assert_eq!(
        kraken(r#"{"error":[],"result":{"A":{"c":[]}}}"#),
        Err(ParseRefused::WrongShape)
    );
    assert_eq!(
        kraken(r#"{"error":[],"result":{"A":{"c":[73554.5]}}}"#),
        Err(ParseRefused::WrongShape)
    );
    assert_eq!(
        kraken(r#"{"error":[],"result":[]}"#),
        Err(ParseRefused::WrongShape)
    );
}

fn coinbase(body: &str) -> Result<u64, ParseRefused> {
    parse_response(src("coinbase"), body.as_bytes())
}

#[test]
fn rt_17_a_currency_other_than_the_sources_is_refused() {
    let usd = r#"{"data":{"amount":"73552.17","base":"BTC","currency":"USD"}}"#;
    assert_eq!(coinbase(usd), Ok(7_355_217));
    for other in ["EUR", "usd", "USDT", ""] {
        let body = format!(r#"{{"data":{{"amount":"73552.17","currency":"{other}"}}}}"#);
        assert_eq!(
            coinbase(&body),
            Err(ParseRefused::WrongCurrency),
            "{other:?}"
        );
    }
    assert_eq!(
        coinbase(r#"{"data":{"amount":"73552.17"}}"#),
        Err(ParseRefused::MissingField)
    );
    assert_eq!(
        coinbase(r#"{"data":{"amount":"73552.17","currency":840}}"#),
        Err(ParseRefused::WrongShape)
    );
}

#[test]
fn rt_20_a_bad_price_inside_a_reply_names_its_refusal() {
    assert_eq!(
        last(r#"{"last":"1e5"}"#),
        Err(ParseRefused::Price(PriceRefused::Exponent))
    );
    assert_eq!(
        last(r#"{"last":"0"}"#),
        Err(ParseRefused::Price(PriceRefused::Zero))
    );
}

//! Tests of the reader: the bounds, the grammar, strings, keys and numbers.

use crate::{parse, JsonError, Limits, Value, MAX_BYTES, MAX_DEPTH, MAX_VALUES};

fn read(s: &str) -> Result<Value, JsonError> {
    parse(s.as_bytes(), &Limits::DEFAULT)
}

#[test]
fn the_default_bounds_are_8_mib_64_levels_and_400_thousand_values() {
    assert_eq!((MAX_BYTES, MAX_DEPTH, MAX_VALUES), (8 << 20, 64, 400_000));
    assert_eq!(Limits::default(), Limits::DEFAULT);
}

#[test]
fn more_bytes_than_the_limit_are_refused_before_they_are_read() {
    let big = vec![b'x'; MAX_BYTES + 1];
    assert_eq!(parse(&big, &Limits::DEFAULT), Err(JsonError::TooLong));
    // Exactly the limit is read (and refused for what it is).
    let at = vec![b' '; MAX_BYTES];
    assert_eq!(parse(&at, &Limits::DEFAULT), Err(JsonError::Malformed));
    let mut ok = b"[1]".to_vec();
    ok.resize(MAX_BYTES, b' ');
    assert!(parse(&ok, &Limits::DEFAULT).is_ok());
    ok.push(b' ');
    assert_eq!(parse(&ok, &Limits::DEFAULT), Err(JsonError::TooLong));
}

#[test]
fn the_limits_are_the_callers() {
    let small = Limits {
        max_bytes: 8,
        max_depth: 2,
        max_values: 3,
    };
    assert!(parse(b"[[1]]", &small).is_ok());
    assert!(parse(b"[[]]", &small).is_ok());
    assert_eq!(parse(b"[[[]]]", &small), Err(JsonError::TooDeep));
    assert_eq!(parse(b"[1,2,3,4]", &small), Err(JsonError::TooLong));
    assert_eq!(parse(b"[1,2,3]", &small), Err(JsonError::TooManyValues));
    assert!(parse(b"[1,2]", &small).is_ok());
}

#[test]
fn nesting_past_64_is_refused() {
    let nested = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert!(read(&nested(MAX_DEPTH)).is_ok());
    assert_eq!(read(&nested(MAX_DEPTH + 1)), Err(JsonError::TooDeep));
    // Unclosed deep input is refused for its depth, not by running out of
    // stack.
    assert_eq!(read(&"[".repeat(1_000_000)), Err(JsonError::TooDeep));
    let objects = format!("{}1{}", r#"{"a":"#.repeat(65), "}".repeat(65));
    assert_eq!(read(&objects), Err(JsonError::TooDeep));
}

#[test]
fn more_values_than_the_limit_are_refused() {
    let many = format!("[{}0]", "0,".repeat(MAX_VALUES));
    assert_eq!(read(&many), Err(JsonError::TooManyValues));
    let fits = format!("[{}0]", "0,".repeat(MAX_VALUES - 2));
    assert!(read(&fits).is_ok());
}

#[test]
fn bytes_that_are_not_utf8_are_refused() {
    assert_eq!(
        parse(&[b'"', 0xff, b'"'], &Limits::DEFAULT),
        Err(JsonError::NotUtf8)
    );
    // An overlong form and a lone surrogate in UTF-8 are not UTF-8.
    assert_eq!(
        parse(&[b'"', 0xc0, 0x80, b'"'], &Limits::DEFAULT),
        Err(JsonError::NotUtf8)
    );
    assert_eq!(
        parse(&[b'"', 0xed, 0xa0, 0x80, b'"'], &Limits::DEFAULT),
        Err(JsonError::NotUtf8)
    );
}

#[test]
fn bytes_after_the_document_are_refused() {
    for bad in ["{}x", "{}{}", "{}\0", "1 2", "[]]"] {
        assert_eq!(read(bad), Err(JsonError::TrailingBytes), "{bad:?}");
    }
    assert_eq!(read("  {} \r\n\t"), Ok(Value::Obj(vec![])));
}

#[test]
fn a_document_that_is_not_json_is_refused() {
    for bad in [
        "",
        " ",
        "{",
        "{\"a\":}",
        "{\"a\" \"1\"}",
        "{a:\"1\"}",
        "{'a':'1'}",
        "{\"a\":\"1\",}",
        "[1,]",
        "[1 2]",
        "{\"a\":01}",
        "{\"a\":1.}",
        "{\"a\":.5}",
        "{\"a\":1e}",
        "{\"a\":-}",
        "{\"a\":+1}",
        "{\"a\":tru}",
        "{\"a\":\"\\x\"}",
        "{\"a\":\"\\u12\"}",
        "{\"a\":\"\\ud800\"}",
        "{\"a\":\"\\udc00\"}",
        "{\"a\":\"\\ud800\\u0041\"}",
        "{\"a\":\"tab\there\"}",
        "{\"a\":\"open}",
        "NaN",
        "Infinity",
    ] {
        assert_eq!(read(bad), Err(JsonError::Malformed), "{bad:?}");
    }
}

#[test]
fn every_kind_of_value_is_read() {
    let doc = r#" {"a":[1,-2.5e+3,0,true,false,null,"x\n\u00e9\ud83d\ude00"],"b":{},"c":[]} "#;
    let v = read(doc).expect("reads");
    let a = v.get("a").and_then(Value::as_array).expect("an array");
    assert_eq!(a.len(), 7);
    assert_eq!(a.first(), Some(&Value::Int(1)));
    assert_eq!(a.get(1), Some(&Value::Number("-2.5e+3".to_string())));
    assert_eq!(a.get(3).and_then(Value::as_bool), Some(true));
    assert_eq!(a.get(4).and_then(Value::as_bool), Some(false));
    assert!(a.get(5).is_some_and(Value::is_null));
    assert_eq!(a.last().and_then(Value::as_str), Some("x\n\u{e9}\u{1f600}"));
    assert_eq!(v.get("b").and_then(Value::as_object), Some(&[][..]));
    assert_eq!(v.get("c").and_then(Value::as_array), Some(&[][..]));
    assert_eq!(v.get("zzz"), None);
    assert_eq!(a.first().and_then(|x| x.get("k")), None);
}

#[test]
fn a_key_twice_in_one_object_is_refused() {
    assert_eq!(read(r#"{"a":1,"a":2}"#), Err(JsonError::DuplicateKey));
    // However it is spelled.
    assert_eq!(read(r#"{"a":1,"\u0061":2}"#), Err(JsonError::DuplicateKey));
    // Off the path too.
    assert_eq!(
        read(r#"{"a":1,"x":{"y":1,"y":2}}"#),
        Err(JsonError::DuplicateKey)
    );
    // The same key in two different objects is fine.
    assert!(read(r#"{"a":1,"x":{"a":1},"y":{"a":2}}"#).is_ok());
}

#[test]
fn an_integer_is_exact_or_it_is_not_one() {
    assert_eq!(read("0"), Ok(Value::Int(0)));
    assert_eq!(read("-1"), Ok(Value::Int(-1)));
    assert_eq!(read("9223372036854775807"), Ok(Value::Int(i64::MAX)));
    assert_eq!(read("-9223372036854775808"), Ok(Value::Int(i64::MIN)));
    assert_eq!(read("9223372036854775808"), Ok(Value::UInt(1 << 63)));
    assert_eq!(read("18446744073709551615"), Ok(Value::UInt(u64::MAX)));
    // Too large for both: a number with no value.
    assert_eq!(
        read("18446744073709551616"),
        Ok(Value::Number("18446744073709551616".to_string()))
    );
    assert_eq!(
        read("-9223372036854775809"),
        Ok(Value::Number("-9223372036854775809".to_string()))
    );
    // A fraction or an exponent is not an integer, whatever it equals.
    for n in ["1.0", "1e2", "1E2", "100e-2", "0.5", "-0", "-0.0"] {
        assert_eq!(read(n), Ok(Value::Number(n.to_string())), "{n}");
    }
}

#[test]
fn a_number_keeps_its_exact_text() {
    let v = read(r#"{"p":0.6,"q":[1.50,2E+2,-0],"i":7}"#).expect("reads");
    assert_eq!(v.get("p").and_then(Value::number_text), Some("0.6"));
    let q = v.get("q").and_then(Value::as_array).expect("an array");
    let texts: Vec<_> = q.iter().map(Value::number_text).collect();
    assert_eq!(texts, [Some("1.50"), Some("2E+2"), Some("-0")]);
    // An exact integer is a value, not text.
    assert_eq!(v.get("i").and_then(Value::number_text), None);
}

#[test]
fn the_integer_readers_refuse_what_is_not_exact_or_not_in_range() {
    let max = read("18446744073709551615").expect("reads");
    assert_eq!(max.as_u64(), Some(u64::MAX));
    assert_eq!(max.as_i64(), None);
    let neg = read("-5").expect("reads");
    assert_eq!(neg.as_i64(), Some(-5));
    assert_eq!(neg.as_u64(), None);
    let pos = read("5").expect("reads");
    assert_eq!((pos.as_i64(), pos.as_u64()), (Some(5), Some(5)));
    for other in ["1.5", "1e3", "-0", "99999999999999999999", "\"5\"", "true"] {
        let v = read(other).expect("reads");
        assert_eq!((v.as_i64(), v.as_u64()), (None, None), "{other}");
    }
}

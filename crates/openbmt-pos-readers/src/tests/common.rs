//! PS-93, PS-94: currencies, ids, text, the cursor and the page size.

use openbmt_pos::{Cursor, MAX_CURSOR_BYTES};

use crate::common::{currency, cut, id, name, note, same_name, split_cursor};
use crate::time::MAX_TIME;
use crate::{cursor_at, ReadRefused, RequestRefused};

use super::text;

#[test]
fn ps_93_a_currency_is_three_capital_letters() {
    assert!(currency("USD", "c").is_ok());
    for bad in ["", "US", "USDX", "usd", "Usd", "U5D", "\u{dc}SD", " USD"] {
        assert_eq!(
            currency(bad, "c"),
            Err(ReadRefused::BadCurrency("c")),
            "{bad:?}"
        );
    }
}

#[test]
fn ps_93_an_id_is_refused_never_cut() {
    assert!(id("abc-123 ~", "i").is_ok());
    assert!(id(&"a".repeat(64), "i").is_ok());
    for bad in [
        String::new(),
        "a".repeat(65),
        "tab\there".to_string(),
        "caf\u{e9}".to_string(),
        "line\nbreak".to_string(),
    ] {
        assert_eq!(id(&bad, "i"), Err(ReadRefused::BadId("i")), "{bad:?}");
    }
}

#[test]
fn ps_93_a_name_is_cut_at_a_character_boundary_and_refused_if_it_has_control_characters() {
    assert_eq!(
        name("Bitcoin", "n").map(|t| t.to_string()),
        Ok("Bitcoin".to_string())
    );
    // 63 bytes, then a two-byte character: the cut falls inside it.
    let long = format!("{}\u{e9}{}", "a".repeat(63), "b".repeat(10));
    let shortened = name(&long, "n").expect("cut");
    assert_eq!(shortened.as_str(), "a".repeat(63));
    assert_eq!(cut(&"\u{1f600}".repeat(20)).len(), 64);
    for bad in ["", "tab\there", "bell\u{7}", "\u{85}"] {
        assert_eq!(name(bad, "n"), Err(ReadRefused::BadText("n")), "{bad:?}");
    }
}

#[test]
fn ps_93_a_note_has_its_control_characters_turned_to_spaces_and_may_be_empty() {
    assert_eq!(
        note("btc K7M2QX").map(|t| t.to_string()),
        Some("btc K7M2QX".to_string())
    );
    assert_eq!(
        note("line\nbreak\tK7M2QX\r").map(|t| t.to_string()),
        Some("line break K7M2QX".to_string())
    );
    assert_eq!(
        note("  padded  ").map(|t| t.to_string()),
        Some("padded".to_string())
    );
    assert_eq!(note(""), None);
    assert_eq!(note(" \n\t "), None);
    // Cut to 64 bytes; a code the cut splits is not found (PS-03).
    let long = format!("{} K7M2QX", "x".repeat(60));
    assert_eq!(
        note(&long).map(|t| t.to_string()),
        Some(format!("{} K7M", "x".repeat(60)))
    );
}

#[test]
fn ps_93_a_name_is_compared_as_the_settings_do() {
    let bitcoin = text("Bitcoin");
    assert!(same_name("Bitcoin", &bitcoin));
    assert!(same_name("  bITCOIN\n", &bitcoin));
    assert!(!same_name("Bitcoin Cash", &bitcoin));
    assert!(!same_name("Bitcoin K7M2QX", &bitcoin));
    assert!(!same_name("Bitcoln", &bitcoin));
    assert!(!same_name("", &bitcoin));
}

#[test]
fn ps_94_a_cursor_is_a_time_then_optionally_a_bar_and_the_vendors_marker() {
    assert_eq!(split_cursor(&Cursor::start()), Ok((0, None)));
    assert_eq!(cursor_at(0).as_ref().map(Cursor::as_bytes), Ok(&b"0"[..]));
    let c = cursor_at(1_794_319_000).expect("a cursor");
    assert_eq!(c.as_bytes(), b"1794319000");
    assert_eq!(split_cursor(&c), Ok((1_794_319_000, None)));
    let with = Cursor::new(b"1794319000|abc_DEF-1.2".to_vec()).expect("a cursor");
    assert_eq!(
        split_cursor(&with),
        Ok((1_794_319_000, Some("abc_DEF-1.2".to_string())))
    );
    // The vendor's marker may hold a bar of its own.
    let bars = Cursor::new(b"5|a|b".to_vec()).expect("a cursor");
    assert_eq!(split_cursor(&bars), Ok((5, Some("a|b".to_string()))));
    assert!(cursor_at(MAX_TIME).is_ok());
    assert_eq!(cursor_at(MAX_TIME + 1), Err(RequestRefused::BadCursor));
}

#[test]
fn ps_94_a_cursor_that_is_not_in_that_form_is_refused() {
    for bad in [
        &b"abc"[..],
        b"|abc",
        b"5|",
        b"05",
        b"+5",
        b"-5",
        b"5 ",
        b"5|a b",
        b"5|a\n",
        b"5|\xc3\xa9",
        b"\xff",
        b"253402300800",
        b"99999999999999999999999",
    ] {
        let c = Cursor::new(bad.to_vec()).expect("short");
        assert_eq!(split_cursor(&c), Err(RequestRefused::BadCursor), "{bad:?}");
    }
}

#[test]
fn ps_94_a_cursor_longer_than_the_model_keeps_is_refused() {
    use crate::square::Request;
    let settings = super::settings();
    let from = Cursor::new(b"1794319000".to_vec()).expect("a cursor");
    let req = Request::new(&settings, 500, &from).expect("a request");
    // A vendor marker that fits only just.
    let fits = "m".repeat(MAX_CURSOR_BYTES - "1794319000|".len());
    let body = format!(r#"{{"cursor":"{fits}"}}"#);
    let page = req.parse(body.as_bytes(), None).expect("fits");
    assert_eq!(page.next.as_bytes().len(), MAX_CURSOR_BYTES);
    let body = format!(r#"{{"cursor":"{fits}m"}}"#);
    assert_eq!(
        req.parse(body.as_bytes(), None),
        Err(ReadRefused::CursorTooLong)
    );
}

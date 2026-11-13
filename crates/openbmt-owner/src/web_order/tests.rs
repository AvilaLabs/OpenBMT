//! Tests, named after the rule each proves.

use openbmt_json::{Limits, Value};

use super::check::{OrderCheck, OrderRefusal, SentRanges, StoreOrder};
use super::vectors::{self, addresses};
use super::*;
use crate::starter::{KernelRandom, KERNEL_BYTES};

/// A kernel that gives the bytes it is told to, in turn.
struct Fixed(Vec<[u8; KERNEL_BYTES]>);

impl KernelRandom for Fixed {
    fn read32(&mut self) -> Option<[u8; KERNEL_BYTES]> {
        if self.0.is_empty() {
            None
        } else {
            Some(self.0.remove(0))
        }
    }
}

fn public() -> [u8; PUBLIC_BYTES] {
    vectors::key().public()
}

fn id() -> [u8; PAIRING_ID_BYTES] {
    pairing_id_of(&vectors::secret())
}

fn order(n: u64, address: &str) -> StoreOrder {
    StoreOrder {
        order: n,
        address: address.to_string(),
        sats: 50_000,
        quoted_at: 1_800_000_000,
        total_minor: 2_500,
        currency: "USD".to_string(),
    }
}

#[test]
fn wo_43_a_key_is_made_from_two_checked_reads() {
    let mut rng = Fixed(vec![vectors::seed(1), vectors::seed(9)]);
    let key = WebOrderKey::generate(&mut rng).expect("a key");
    assert_eq!(key.seed(), vectors::seed(1));
    assert_eq!(key.public(), public());
}

#[test]
fn wo_43_weak_repeated_or_missing_bytes_make_no_key() {
    for reads in [
        vec![[0u8; 32], vectors::seed(9)],
        vec![vectors::seed(1), vectors::seed(1)],
        vec![vectors::seed(1)],
        vec![],
    ] {
        assert_eq!(
            WebOrderKey::generate(&mut Fixed(reads)).unwrap_err(),
            WebOrderRefusal::Random
        );
    }
}

#[test]
fn wo_43_the_debug_text_of_a_key_holds_no_byte_of_it() {
    assert_eq!(format!("{:?}", vectors::key()), "WebOrderKey(..)");
}

#[test]
fn wo_05_the_fingerprint_is_four_groups_of_four() {
    let text = vectors::key().fingerprint();
    let groups: Vec<&str> = text.split('-').collect();
    assert_eq!(groups.len(), 4);
    assert!(groups
        .iter()
        .all(|g| g.len() == 4 && g.bytes().all(|b| b.is_ascii_hexdigit())));
    assert_eq!(WebOrderKey::fingerprint_of(&public()), text);
}

#[test]
fn wo_05_a_code_holds_the_key_the_secret_and_the_name_and_reads_back() {
    let code = PairingCode::new(public(), vectors::secret(), vectors::SHOP_NAME).expect("a code");
    let bytes = code.to_bytes();
    assert_eq!(bytes.get(..4), Some(&PAIRING_MAGIC[..]));
    assert_eq!(PairingCode::from_bytes(&bytes).expect("read"), code);
    assert_eq!(PairingCode::from_text(&code.to_text()).expect("read"), code);
    assert_eq!(code.public(), &public());
    assert_eq!(code.secret(), &vectors::secret());
    assert_eq!(code.shop_name(), vectors::SHOP_NAME);
    assert_eq!(code.fingerprint(), vectors::key().fingerprint());
    // White space around the text is ignored.
    let spaced = format!(" \n{}\r\n", code.to_text());
    assert_eq!(PairingCode::from_text(&spaced).expect("read"), code);
}

#[test]
fn wo_05_a_generated_code_takes_a_fresh_secret() {
    let key = vectors::key();
    let mut rng = Fixed(vec![vectors::seed(0x40), vectors::seed(0x50)]);
    let code = PairingCode::generate(&key, "Shop", &mut rng).expect("a code");
    assert_eq!(code.secret(), &vectors::seed(0x40));
    let mut weak = Fixed(vec![[7u8; 32], vectors::seed(1)]);
    assert_eq!(
        PairingCode::generate(&key, "Shop", &mut weak).unwrap_err(),
        WebOrderRefusal::Random
    );
}

#[test]
fn wo_05_the_reader_refuses_every_changed_or_cut_code() {
    let code = PairingCode::new(public(), vectors::secret(), "Shop").expect("a code");
    let bytes = code.to_bytes();
    for at in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[at] ^= 1;
        assert!(PairingCode::from_bytes(&changed).is_err(), "byte {at}");
        assert!(PairingCode::from_bytes(&bytes[..at]).is_err(), "cut {at}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(PairingCode::from_bytes(&longer).is_err());
}

#[test]
fn wo_05_a_name_that_is_empty_long_or_has_a_control_byte_is_refused() {
    let make = |name: &str| PairingCode::new(public(), vectors::secret(), name);
    assert!(make("").is_err());
    assert!(make(&"a".repeat(MAX_SHOP_NAME_BYTES)).is_ok());
    assert!(make(&"a".repeat(MAX_SHOP_NAME_BYTES + 1)).is_err());
    assert!(make("two\nlines").is_err());
    assert!(make("tab\there").is_err());
    assert!(make("del\u{7f}").is_err());
}

#[test]
fn wo_05_text_with_other_characters_or_loose_bits_is_refused() {
    let code = PairingCode::new(public(), vectors::secret(), "Shop").expect("a code");
    let text = code.to_text();
    assert!(PairingCode::from_text(&text.to_lowercase()).is_err());
    assert!(PairingCode::from_text(&format!("{text}A")).is_err());
    assert!(PairingCode::from_text(&format!("{text}=")).is_err());
    assert!(PairingCode::from_text(&text.replacen('A', "1", 1).replace('B', "8")).is_err());
    assert!(PairingCode::from_text("").is_err());
    assert!(PairingCode::from_text(&text[..text.len() - 1]).is_err());
}

#[test]
fn wo_10_a_range_is_signed_and_opens_under_the_key() {
    let range = vectors::range();
    let frame = range.sign(&vectors::key(), &id());
    assert_eq!(frame.get(..4), Some(&RANGE_MAGIC[..]));
    assert_eq!(Range::open(&frame, &public(), &id()).expect("opens"), range);
    // The same range signed twice is the same bytes.
    assert_eq!(frame, range.sign(&vectors::key(), &id()));
}

#[test]
fn wo_10_a_range_of_one_hundred_is_allowed_and_one_more_is_not() {
    assert!(Range::new(0, addresses(RANGE_SIZE as u32)).is_ok());
    assert!(Range::new(0, addresses(RANGE_SIZE as u32 + 1)).is_err());
    assert!(Range::new(0, vec![]).is_err());
    // The indexes of the range must fit.
    assert!(Range::new(u32::MAX - 2, addresses(3)).is_err());
    assert!(Range::new(u32::MAX - 3, addresses(3)).is_ok());
}

#[test]
fn wo_10_an_address_that_is_not_a_lower_case_segwit_address_is_refused() {
    let good = addresses(1).remove(0);
    assert!(Range::new(0, vec![good.clone()]).is_ok());
    for bad in [
        good.to_uppercase(),
        format!("{good} "),
        format!("{}x", &good[..good.len() - 1]),
        "1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2".to_string(),
        "bc1".to_string(),
        String::new(),
    ] {
        assert!(Range::new(0, vec![bad.clone()]).is_err(), "{bad}");
    }
}

#[test]
fn wo_10_every_changed_byte_of_a_signed_range_is_refused() {
    let frame = vectors::range().sign(&vectors::key(), &id());
    for at in 0..frame.len() {
        let mut changed = frame.clone();
        changed[at] ^= 1;
        assert!(
            Range::open(&changed, &public(), &id()).is_err(),
            "byte {at}"
        );
    }
    for cut in 0..frame.len() {
        assert!(
            Range::open(&frame[..cut], &public(), &id()).is_err(),
            "cut {cut}"
        );
    }
    let mut longer = frame.clone();
    longer.push(0);
    assert!(Range::open(&longer, &public(), &id()).is_err());
}

#[test]
fn wo_10_a_range_signed_by_another_key_is_refused() {
    let other = WebOrderKey::from_seed(&vectors::seed(0x80));
    let frame = vectors::range().sign(&other, &id());
    assert_eq!(
        Range::open(&frame, &public(), &id()).unwrap_err(),
        WebOrderRefusal::Signature
    );
}

#[test]
fn wo_10_a_range_of_another_pairing_is_refused() {
    let other = pairing_id_of(&vectors::seed(0x90));
    assert_ne!(other, id());
    let frame = vectors::range().sign(&vectors::key(), &other);
    assert_eq!(
        Range::open(&frame, &public(), &id()).unwrap_err(),
        WebOrderRefusal::PairingId
    );
    assert!(Range::open(&frame, &public(), &other).is_ok());
}

#[test]
fn wo_10_the_id_of_a_pairing_is_the_start_of_the_hash_of_its_secret() {
    let code = PairingCode::new(public(), vectors::secret(), "Shop").expect("a code");
    assert_eq!(code.id(), id());
    let again = PairingCode::new(public(), vectors::seed(0x41), "Shop").expect("a code");
    assert_ne!(again.id(), id());
}

#[test]
fn wo_12_a_range_is_remembered_and_never_sent_twice() {
    let mut sent = SentRanges::new();
    let first = Range::new(0, addresses(3)).expect("a range");
    sent.add_range(&first).expect("added");
    assert_eq!(sent.sent_count(), 3);
    assert!(sent.add_range(&first).is_err());
    assert!(sent
        .add_range(&Range::new(2, addresses(3)).expect("a range"))
        .is_err());
    assert_eq!(sent.sent_count(), 3);
}

#[test]
fn wo_20_a_request_has_three_headers_that_follow_from_its_parts() {
    let (method, route, time, nonce, body) = vectors::request_inputs();
    let one = SignedRequest::make(&vectors::secret(), method, route, time, nonce, &body)
        .expect("a request");
    let again = SignedRequest::make(&vectors::secret(), method, route, time, nonce, &body)
        .expect("a request");
    assert_eq!(one, again);
    assert_eq!(one.time_text(), "1800000000");
    assert_eq!(one.nonce_text().len(), 2 * NONCE_BYTES);
    assert_eq!(one.mac_text().len(), 64);
    let differ = |other: Result<SignedRequest, WebOrderRefusal>| {
        assert_ne!(other.expect("a request").mac, one.mac);
    };
    differ(SignedRequest::make(
        &vectors::secret(),
        "GET",
        route,
        time,
        nonce,
        &body,
    ));
    differ(SignedRequest::make(
        &vectors::secret(),
        method,
        ROUTE_NOTICE,
        time,
        nonce,
        &body,
    ));
    differ(SignedRequest::make(
        &vectors::secret(),
        method,
        route,
        time + 1,
        nonce,
        &body,
    ));
    differ(SignedRequest::make(
        &vectors::secret(),
        method,
        route,
        time,
        [0; 16],
        &body,
    ));
    differ(SignedRequest::make(
        &vectors::secret(),
        method,
        route,
        time,
        nonce,
        b"",
    ));
    differ(SignedRequest::make(
        &vectors::seed(0x41),
        method,
        route,
        time,
        nonce,
        &body,
    ));
}

#[test]
fn wo_20_a_method_or_route_that_could_change_the_message_is_refused() {
    let secret = vectors::secret();
    for (method, route) in [
        ("", "/a"),
        ("post", "/a"),
        ("PO\nST", "/a"),
        ("POST", "a"),
        ("POST", "/a\n/b"),
        ("POST", "/a b"),
    ] {
        assert_eq!(
            SignedRequest::make(&secret, method, route, 1, [0; 16], b"").unwrap_err(),
            WebOrderRefusal::Request,
            "{method:?} {route:?}"
        );
    }
}

#[test]
fn wo_20_the_number_used_once_is_the_first_16_bytes_of_a_read() {
    let mut rng = Fixed(vec![vectors::seed(3)]);
    let nonce = SignedRequest::nonce_from(&mut rng).expect("a nonce");
    assert_eq!(nonce[..], vectors::seed(3)[..NONCE_BYTES]);
    assert_eq!(
        SignedRequest::nonce_from(&mut Fixed(vec![])).unwrap_err(),
        WebOrderRefusal::Random
    );
}

#[test]
fn wo_20_the_window_is_five_minutes() {
    assert_eq!(TIME_WINDOW_SECS, 300);
    assert_eq!(orders_request_body(41), "{\"since\":41}");
}

#[test]
fn wo_20_a_mac_made_by_the_plugin_is_the_one_made_here() {
    let text = std::fs::read_to_string(vectors::folder().join("php_hmac.json")).expect("the file");
    let doc = openbmt_json::parse(text.as_bytes(), &Limits::DEFAULT).expect("json");
    let field = |k: &str| doc.get(k).and_then(Value::as_str).expect(k).to_string();
    let secret: [u8; 32] = unhex(&field("secret"))
        .expect("hex")
        .try_into()
        .expect("32");
    let nonce: [u8; NONCE_BYTES] = unhex(&field("nonce")).expect("hex").try_into().expect("16");
    let time = doc.get("time").and_then(Value::as_u64).expect("time");
    let made = SignedRequest::make(
        &secret,
        &field("method"),
        &field("route"),
        time,
        nonce,
        field("body").as_bytes(),
    )
    .expect("a request");
    assert_eq!(made.mac_text(), field("mac"));
}

#[test]
fn wo_20_the_orders_a_store_reports_are_read_strictly() {
    let text = br#"{"orders":[{"order":5,"address":"bc1qq","sats":900,"quoted_at":1800000000,"total_minor":2500,"currency":"USD"}]}"#;
    let list = StoreOrder::parse_list(text).expect("a list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].order, 5);
    assert_eq!(list[0].currency, "USD");
    assert!(StoreOrder::parse_list(br#"{"orders":[]}"#)
        .expect("empty")
        .is_empty());
    for bad in [
        &b""[..],
        b"[]",
        br#"{"orders":{}}"#,
        br#"{"orders":[{"order":-1,"address":"a","sats":1,"quoted_at":1,"total_minor":1,"currency":"USD"}]}"#,
        br#"{"orders":[{"order":1,"address":"a","sats":1,"quoted_at":1,"total_minor":1}]}"#,
        br#"{"orders":[{"order":1,"address":"a","sats":1.5,"quoted_at":1,"total_minor":1,"currency":"USD"}]}"#,
        br#"{"orders":[]} x"#,
    ] {
        assert_eq!(
            StoreOrder::parse_list(bad).unwrap_err(),
            WebOrderRefusal::Orders,
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
}

#[test]
fn wo_22_an_order_for_an_address_sent_is_taken_once() {
    let mut sent = SentRanges::new();
    let list = addresses(3);
    sent.add_range(&Range::new(0, list.clone()).expect("a range"))
        .expect("added");
    assert_eq!(sent.check(&order(1, &list[0])), Ok(OrderCheck::New));
    assert_eq!(sent.check(&order(2, &list[1])), Ok(OrderCheck::New));
    // The same order again, on the next visit.
    assert_eq!(sent.check(&order(1, &list[0])), Ok(OrderCheck::Known));
}

#[test]
fn wo_22_an_address_not_in_a_range_sent_is_refused() {
    let mut sent = SentRanges::new();
    let list = addresses(4);
    sent.add_range(&Range::new(0, list[..3].to_vec()).expect("a range"))
        .expect("added");
    assert_eq!(
        sent.check(&order(1, &list[3])),
        Err(OrderRefusal::AddressNotSent)
    );
    // A refusal records nothing: the order number is free again.
    assert_eq!(sent.check(&order(1, &list[0])), Ok(OrderCheck::New));
}

#[test]
fn wo_22_an_address_used_before_by_another_order_is_refused() {
    let mut sent = SentRanges::new();
    let list = addresses(2);
    sent.add_range(&Range::new(0, list.clone()).expect("a range"))
        .expect("added");
    assert_eq!(sent.check(&order(1, &list[0])), Ok(OrderCheck::New));
    assert_eq!(
        sent.check(&order(2, &list[0])),
        Err(OrderRefusal::AddressUsedBefore)
    );
}

#[test]
fn wo_22_an_order_that_changes_its_address_or_has_no_amount_is_refused() {
    let mut sent = SentRanges::new();
    let list = addresses(2);
    sent.add_range(&Range::new(0, list.clone()).expect("a range"))
        .expect("added");
    assert_eq!(sent.check(&order(1, &list[0])), Ok(OrderCheck::New));
    assert_eq!(
        sent.check(&order(1, &list[1])),
        Err(OrderRefusal::OrderChanged)
    );
    let mut free = order(2, &list[1]);
    free.sats = 0;
    assert_eq!(sent.check(&free), Err(OrderRefusal::NoAmount));
}

#[test]
fn wo_30_a_notice_is_signed_and_opens_under_the_key() {
    let notice = vectors::notice();
    let frame = notice.sign(&vectors::key()).expect("signed");
    assert_eq!(frame.get(..4), Some(&NOTICE_MAGIC[..]));
    assert_eq!(Notice::open(&frame, &public()).expect("opens"), notice);
}

#[test]
fn wo_30_every_state_has_its_own_byte_and_reads_back() {
    for state in [
        NoticeState::Paid,
        NoticeState::Overpaid,
        NoticeState::PartPaid,
        NoticeState::PaidLate,
    ] {
        let mut n = vectors::notice();
        n.state = state;
        let frame = n.sign(&vectors::key()).expect("signed");
        assert_eq!(Notice::open(&frame, &public()).expect("opens").state, state);
    }
    assert_eq!(NoticeState::Paid.byte(), 1);
    assert_eq!(NoticeState::PaidLate.byte(), 4);
}

#[test]
fn wo_30_every_changed_byte_of_a_signed_notice_is_refused() {
    let frame = vectors::notice().sign(&vectors::key()).expect("signed");
    for at in 0..frame.len() {
        let mut changed = frame.clone();
        changed[at] ^= 1;
        assert!(Notice::open(&changed, &public()).is_err(), "byte {at}");
    }
    for cut in 0..frame.len() {
        assert!(Notice::open(&frame[..cut], &public()).is_err(), "cut {cut}");
    }
    let mut longer = frame.clone();
    longer.push(0);
    assert!(Notice::open(&longer, &public()).is_err());
}

#[test]
fn wo_30_a_notice_signed_by_another_key_is_refused() {
    let other = WebOrderKey::from_seed(&vectors::seed(0x80));
    let frame = vectors::notice().sign(&other).expect("signed");
    assert_eq!(
        Notice::open(&frame, &public()).unwrap_err(),
        WebOrderRefusal::Signature
    );
}

#[test]
fn wo_30_a_notice_with_no_transaction_or_too_many_is_not_made() {
    let key = vectors::key();
    let mut n = vectors::notice();
    n.txids.clear();
    assert!(n.sign(&key).is_err());
    n.txids = vec![vectors::txid(1); MAX_NOTICE_TXIDS];
    assert!(n.sign(&key).is_ok());
    n.txids = vec![vectors::txid(1); MAX_NOTICE_TXIDS + 1];
    assert!(n.sign(&key).is_err());
    let mut bad_address = vectors::notice();
    bad_address.address = "not an address".to_string();
    assert!(bad_address.sign(&key).is_err());
}

#[test]
fn wo_50_the_committed_vectors_are_the_ones_the_code_makes() {
    let path = vectors::folder().join("web_order.json");
    let text = vectors::vector_file();
    if std::env::var_os("OPENBMT_BLESS_WEB_ORDER").is_some() {
        std::fs::create_dir_all(vectors::folder()).expect("a directory");
        std::fs::write(&path, &text).expect("written");
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == text,
        "the vector file differs from the code: run the tests of this crate with \
         OPENBMT_BLESS_WEB_ORDER=1 to write it again"
    );
}

#[test]
fn wo_50_the_vector_file_is_json_with_every_part() {
    let text = vectors::vector_file();
    let doc = openbmt_json::parse(text.as_bytes(), &Limits::DEFAULT).expect("json");
    for key in [
        "key",
        "pairing",
        "range",
        "notice",
        "request",
        "request_tampered",
    ] {
        assert!(doc.get(key).is_some(), "{key}");
    }
    let tampered = doc
        .get("range")
        .and_then(|r| r.get("tampered"))
        .and_then(Value::as_array)
        .expect("a list");
    assert!(tampered.len() >= 8);
}

#[test]
fn wo_50_every_tampered_vector_is_refused_here_too() {
    let text = vectors::vector_file();
    let doc = openbmt_json::parse(text.as_bytes(), &Limits::DEFAULT).expect("json");
    let frames = |part: &str| -> Vec<(String, Vec<u8>)> {
        doc.get(part)
            .and_then(|p| p.get("tampered"))
            .and_then(Value::as_array)
            .expect("a list")
            .iter()
            .map(|t| {
                let name = t.get("name").and_then(Value::as_str).expect("name");
                let frame = t.get("frame").and_then(Value::as_str).expect("frame");
                (name.to_string(), unhex(frame).expect("hex"))
            })
            .collect()
    };
    for (name, frame) in frames("range") {
        assert!(
            Range::open(&frame, &public(), &id()).is_err(),
            "range {name}"
        );
    }
    for (name, frame) in frames("notice") {
        assert!(Notice::open(&frame, &public()).is_err(), "notice {name}");
    }
}

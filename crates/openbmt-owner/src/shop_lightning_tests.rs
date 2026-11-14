use super::*;

/// The generator point, as a compressed key: a real point on the curve.
const KEY: &str = "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

#[test]
fn sl_76_a_node_id_is_66_lower_case_digits_of_a_compressed_key() {
    let id = parse_node_id(KEY).unwrap();
    assert_eq!(id[0], 2);
    assert_eq!(id.len(), LIGHTNING_NODE_BYTES);
    for bad in [
        "",
        &KEY[..64],
        &format!("{KEY}00"),
        &KEY.to_ascii_uppercase(),
        &format!("{KEY}@host:9735"),
        &format!(" {KEY}"),
        // 04 is not a compressed point.
        &format!("04{}", &KEY[2..]),
        // 66 digits that are not a point of the curve.
        &format!("02{}", "00".repeat(32)),
        &KEY.replace('7', "g"),
    ] {
        assert_eq!(
            parse_node_id(bad),
            Err(ShopLightningRefusal::NodeId),
            "{bad}"
        );
    }
}

#[test]
fn sl_71_the_provider_key_reads_capitals_and_the_address_needs_a_port() {
    assert_eq!(
        parse_provider_key(&KEY.to_ascii_uppercase()).unwrap(),
        parse_node_id(KEY).unwrap()
    );
    assert_eq!(
        parse_provider_key("02ab"),
        Err(ShopLightningRefusal::ProviderKey)
    );
    assert_eq!(
        parse_provider_addr("203.0.113.7:9735").unwrap().port(),
        9735
    );
    assert!(parse_provider_addr("[2001:db8::1]:9735").unwrap().is_ipv6());
    for bad in [
        "",
        "203.0.113.7",
        "203.0.113.7:0",
        "host.example:9735",
        "1.2.3.4:70000",
    ] {
        assert_eq!(
            parse_provider_addr(bad),
            Err(ShopLightningRefusal::ProviderAddress),
            "{bad}"
        );
    }
}

/// SC-12: every refusal has written words that pass the checks, carry the
/// rule they are from, and read apart from one another.
#[test]
fn sc_12_the_words_pass_the_checks_carry_their_rule_and_differ() {
    let all = all_refusals();
    assert_eq!(all.len(), ShopLightningRefusal::ALL.len());
    for (name, w) in &all {
        assert_eq!(openbmt_core::say::check(w), Ok(()), "{name}");
        assert!(!w.heading.contains("WORDS_NEEDED"), "{name}");
    }
    assert_eq!(openbmt_core::say::check_distinct(&all), Ok(()));
    for r in ShopLightningRefusal::ALL {
        assert_eq!(r.words().code, Some(r.rule()), "{r:?}");
    }
}

//! Reading a deposit code: a bare address or a BIP21 URI.

use bitcoin::Address;

use super::kit::*;
use crate::{parse_deposit_code, DepositCode, KeyError, KeyNetwork};

/// A P2PKH address on mainnet. (The address in the BIP21 text itself has a
/// wrong checksum, so the examples are run with this one.)
const MAIN: &str = "1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2";
const BECH: &str = "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq";
const TAPROOT: &str = "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0";

fn parse(text: &str) -> Result<DepositCode, KeyError> {
    parse_deposit_code(text.to_string(), KeyNetwork::Mainnet)
}

fn ok(text: &str) -> DepositCode {
    parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn rule(text: &str) -> String {
    parse(text).expect_err(text).rule().to_string()
}

fn amount(a: &str) -> Result<Option<u64>, KeyError> {
    parse(&format!("bitcoin:{MAIN}?amount={a}")).map(|c| c.amount_sats)
}

#[test]
fn a_bare_address_is_read() {
    let c = ok(MAIN);
    assert_eq!(c.address, MAIN);
    assert_eq!(c.amount_sats, None);
    assert_eq!(c.label, None);
    assert!(c.ignored.is_empty());
    assert_eq!(ok(&format!("  {BECH}\n")).address, BECH);
    assert_eq!(ok(TAPROOT).address, TAPROOT);
    assert_eq!(ok(&format!("bitcoin:{MAIN}")), c);
}

#[test]
fn the_bip21_examples() {
    let c = ok(&format!("bitcoin:{MAIN}?label=Luke-Jr"));
    assert_eq!(c.label.as_deref(), Some("Luke-Jr"));
    assert_eq!(c.amount_sats, None);

    let c = ok(&format!("bitcoin:{MAIN}?amount=20.3&label=Luke-Jr"));
    assert_eq!(c.amount_sats, Some(2_030_000_000));
    assert_eq!(c.label.as_deref(), Some("Luke-Jr"));

    let c = ok(&format!(
        "bitcoin:{MAIN}?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz"
    ));
    assert_eq!(c.amount_sats, Some(5_000_000_000));
    assert!(c.ignored.is_empty());

    // An unknown parameter that is not required is listed and not acted on.
    let c = ok(&format!(
        "bitcoin:{MAIN}?somethingyoudontunderstand=50&somethingelseyoudontget=999"
    ));
    assert_eq!(
        c.ignored,
        vec!["somethingyoudontunderstand", "somethingelseyoudontget"]
    );
    assert_eq!(c.amount_sats, None);

    // A parameter that is required and not understood refuses the code.
    assert_eq!(
        rule(&format!(
            "bitcoin:{MAIN}?req-somethingyoudontunderstand=50&req-somethingelseyoudontget=999"
        )),
        "deposit-required"
    );
    assert_eq!(
        rule(&format!(
            "bitcoin:{MAIN}?amount=1&req-somethingyetunknown=1"
        )),
        "deposit-required"
    );
    assert_eq!(rule(&format!("bitcoin:{MAIN}?REQ-X=1")), "deposit-required");
}

#[test]
fn lightning_and_payjoin_are_listed_and_never_acted_on() {
    let c = ok(&format!(
        "bitcoin:{BECH}?amount=0.001&lightning=lnbc1abc&pj=https%3A%2F%2Fexample.com%2Fpj"
    ));
    assert_eq!(c.ignored, vec!["lightning", "pj"]);
    assert_eq!(c.amount_sats, Some(100_000));
    assert_eq!(c.address, BECH);
}

#[test]
fn upper_case_codes_are_read() {
    let c = ok(&format!(
        "BITCOIN:{}?AMOUNT=1&LABEL=Shop",
        BECH.to_uppercase()
    ));
    // The usual spelling is returned.
    assert_eq!(c.address, BECH);
    assert_eq!(c.amount_sats, Some(100_000_000));
    assert_eq!(c.label.as_deref(), Some("Shop"));
    assert_eq!(ok(&format!("Bitcoin:{MAIN}")).address, MAIN);
    // Mixed-case bech32 is not an address.
    let mixed = format!("{}{}", &BECH[..10], BECH[10..].to_uppercase());
    assert_eq!(rule(&mixed), "address");
}

#[test]
fn amounts() {
    assert_eq!(amount("0.00000001").expect("one sat"), Some(1));
    assert_eq!(
        amount("21000000").expect("the most"),
        Some(2_100_000_000_000_000)
    );
    assert_eq!(
        amount("21000000.00000000").expect("the most"),
        Some(2_100_000_000_000_000)
    );
    assert_eq!(amount("0.1").expect("tenth"), Some(10_000_000));
    assert_eq!(amount("1.5").expect("one and a half"), Some(150_000_000));
    assert_eq!(amount("0.12345678").expect("eight"), Some(12_345_678));
    for wrong in [
        "21000000.00000001",
        "21000001",
        "99999999",
        "0.123456789",
        "1e3",
        "1E3",
        "-1",
        "-0.5",
        "+1",
        "",
        ".5",
        "1.",
        ".",
        "0",
        "0.00000000",
        "1,5",
        "0x10",
        "1.2.3",
        "NaN",
        "1%20",
        "123456789",
        "184467440737095516150",
    ] {
        let e = amount(wrong).expect_err(wrong);
        assert_eq!(e.rule(), "deposit-amount", "{wrong:?}");
    }
}

#[test]
fn syntax_limits_and_escapes() {
    assert_eq!(
        rule(&format!("bitcoin:{MAIN}?amount=1&amount=2")),
        "deposit-duplicate"
    );
    assert_eq!(
        rule(&format!("bitcoin:{MAIN}?foo=1&FOO=2")),
        "deposit-duplicate"
    );
    assert_eq!(rule(&format!("bitcoin:{MAIN}?label=a%2")), "deposit-escape");
    assert_eq!(
        rule(&format!("bitcoin:{MAIN}?label=a%zz")),
        "deposit-escape"
    );
    assert_eq!(rule(&format!("bitcoin:{MAIN}?label=%ff")), "deposit-escape");
    assert_eq!(rule(&format!("bitcoin:{MAIN}?fo%o=1")), "deposit-escape");
    assert_eq!(rule(&format!("bitcoin:{MAIN}?")), "deposit-syntax");
    assert_eq!(rule(&format!("bitcoin:{MAIN}?label")), "deposit-syntax");
    assert_eq!(rule(&format!("bitcoin:{MAIN}?=1")), "deposit-syntax");
    assert_eq!(rule(&format!("bitcoin:{MAIN}?a=1&&b=2")), "deposit-syntax");
    assert_eq!(rule(&format!("bitcoin:{MAIN}?label=a b")), "deposit-chars");
    assert_eq!(
        rule(&format!("bitcoin:{MAIN}?label=caf\u{e9}")),
        "deposit-chars"
    );
    assert_eq!(rule(&format!("bitcoin:{MAIN}?label=%0A")), "deposit-label");
    assert_eq!(rule(&format!("bitcoin://{MAIN}")), "address");
    assert_eq!(rule(""), "address");
    assert_eq!(rule("bitcoin:"), "address");
    assert_eq!(rule("not an address"), "deposit-chars");
    assert_eq!(rule("lightning:lnbc1abc"), "address");

    // Percent escapes decode to the label, a plus stays a plus.
    let c = ok(&format!("bitcoin:{MAIN}?label=Caf%C3%A9%20A+B"));
    assert_eq!(c.label.as_deref(), Some("Caf\u{e9} A+B"));

    // The text limit is 2,048 bytes.
    let pad = |n: usize| format!("bitcoin:{MAIN}?x={}", "a".repeat(n));
    let base = pad(0).len();
    assert!(parse(&pad(2_048 - base)).is_ok());
    assert_eq!(rule(&pad(2_049 - base)), "deposit-too-long");
}

#[test]
fn the_address_must_fit_the_network_and_the_allowlist() {
    let (_, s1) = fixed_key(1, 1);
    drop(s1);
    let regtest = Address::from_script(&external(7), NET)
        .expect("an address")
        .to_string();
    assert!(regtest.starts_with("bcrt1"));
    let c = parse_deposit_code(format!("bitcoin:{regtest}"), KeyNetwork::Regtest).expect("fits");
    assert_eq!(c.address, regtest);
    // Wrong network, both ways.
    let e = parse_deposit_code(format!("bitcoin:{regtest}"), KeyNetwork::Mainnet)
        .expect_err("other network");
    assert_eq!(e.rule(), "address");
    let e = parse_deposit_code(format!("bitcoin:{MAIN}"), KeyNetwork::Regtest)
        .expect_err("other network");
    assert_eq!(e.rule(), "address");
    let testnet = "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx";
    assert!(parse_deposit_code(testnet.to_string(), KeyNetwork::Testnet).is_ok());
    // Mainnet kinds the signer takes (SR-22, SR-30).
    for kind in [
        MAIN,
        "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy",
        BECH,
        "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3",
        TAPROOT,
    ] {
        assert_eq!(ok(kind).address, kind);
    }
    // Kinds the allowlist does not take: unknown witness versions.
    for kind in ["bc1zw508d6qejxtdg4y5r3zarvaryvaxxpcs", "bc1sw50qgdz25j"] {
        let e = parse(kind).expect_err(kind);
        assert_eq!(e.rule(), "address-kind", "{kind}");
        assert!(e.text().starts_with("SR-30"));
    }
}

#[test]
fn what_is_read_is_what_enroll_destination_takes() {
    let rig = rig();
    let regtest = Address::from_script(&external(9), NET)
        .expect("an address")
        .to_string();
    let code = parse_deposit_code(
        format!("BITCOIN:{regtest}?label=Till&pj=x"),
        KeyNetwork::Regtest,
    )
    .expect("read");
    // Nothing was enrolled by reading.
    assert!(rig
        .app
        .signer_status()
        .expect("status")
        .allowlist
        .is_empty());
    rig.app
        .enroll_destination(code.address.clone(), code.label.clone().expect("label"))
        .expect("enrolled");
    let status = rig.app.signer_status().expect("status");
    assert_eq!(status.allowlist.len(), 1);
    assert_eq!(
        status.allowlist[0].address.as_deref(),
        Some(code.address.as_str())
    );
}

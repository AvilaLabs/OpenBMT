//! LN-05, LN-08, LN-11, LN-12, LN-23 (credentials in `Debug`).

use bitcoin::hashes::{sha256, Hash};

use super::*;
use crate::credential::{check_rune, MAX_MACAROON_BYTES};
use crate::error::CredentialRefused as R;

#[test]
fn ln_05_a_macaroon_has_the_shape_of_one() {
    assert!(LndCredential::new(endpoint(), &macaroon()).is_ok());
    assert_eq!(
        LndCredential::new(endpoint(), &[]).unwrap_err(),
        R::BadMacaroon
    );
    // Not the binary form of version 2.
    assert_eq!(
        LndCredential::new(endpoint(), &[1, 2, 3]).unwrap_err(),
        R::BadMacaroon
    );
    let mut long = vec![2u8];
    long.extend(vec![0; MAX_MACAROON_BYTES]);
    assert_eq!(
        LndCredential::new(endpoint(), &long).unwrap_err(),
        R::BadMacaroon
    );
    assert!(crate::LND_BAKE_COMMAND.ends_with("invoices:read invoices:write"));
}

fn refused(restrictions: &str) -> R {
    check_rune(&rune(restrictions)).unwrap_err()
}

#[test]
fn ln_08_a_rune_limited_to_the_two_invoice_methods_is_taken() {
    for r in [
        "method=invoice|method=listinvoices",
        "=7&method=invoice|method=listinvoices",
        "method=listinvoices|method=invoice&time<1900000000",
        "id=02abc&method=invoice|method=listinvoices&rate=60",
        // Two restrictions of the method that agree.
        "method=invoice|method=listinvoices&method=listinvoices|method=invoice",
        // A value that holds an escaped bar and ampersand, in a field that
        // does not matter.
        "pnamelabel=a\\|b\\&c&method=invoice|method=listinvoices",
        // Comments and the like only narrow.
        "#remark&method=invoice|method=listinvoices",
    ] {
        assert_eq!(check_rune(&rune(r)), Ok(()), "{r}");
    }
}

#[test]
fn ln_08_a_rune_that_allows_another_method_is_refused() {
    assert_eq!(
        refused("method=invoice|method=listinvoices|method=pay"),
        R::RuneAllowsMethod("pay".to_string())
    );
    assert_eq!(
        refused("method=pay"),
        R::RuneAllowsMethod("pay".to_string())
    );
    // Even when another restriction would narrow it again.
    assert_eq!(
        refused("method=invoice|method=listinvoices|method=pay&method=invoice|method=listinvoices"),
        R::RuneAllowsMethod("pay".to_string())
    );
}

#[test]
fn ln_08_a_rune_that_does_not_limit_the_method_is_refused() {
    // Only other restrictions.
    assert_eq!(refused("time<1900000000"), R::RuneLacksMethod);
    assert_eq!(refused("=3"), R::RuneLacksMethod);
    // Both methods are needed.
    assert_eq!(refused("method=invoice"), R::RuneLacksMethod);
    assert_eq!(refused("method=listinvoices"), R::RuneLacksMethod);
    // Two restrictions that leave nothing between them.
    assert_eq!(
        refused("method=invoice&method=listinvoices"),
        R::RuneLacksMethod
    );
    // No restriction at all: 32 bytes and nothing else.
    let bare = base64(&[0xab; 32], Alphabet::Url);
    assert_eq!(check_rune(&bare), Err(R::RuneUnrestricted));
}

#[test]
fn ln_08_a_restriction_that_can_widen_the_method_is_refused() {
    for r in [
        // A bar makes it as wide as the widest alternative.
        "method=invoice|method=listinvoices|id=02abc",
        "method=invoice|method^list",
        "method^inv",
        "method~invoice",
        "method/pay",
        "method!pay",
        "method=invoice|method=listinvoices|method#",
    ] {
        assert_eq!(refused(r), R::RuneMixesMethod, "{r}");
    }
}

#[test]
fn ln_08_syntax_that_is_not_known_is_refused() {
    for r in [
        // No operator.
        "method",
        // An operator that is not one.
        "method?invoice",
        "method invoice",
        // An escape that is not one.
        "a=b\\n&method=invoice|method=listinvoices",
        "a=b\\",
        // An empty alternative.
        "method=invoice||method=listinvoices",
        "method=invoice|method=listinvoices&",
    ] {
        assert!(matches!(refused(r), R::RuneSyntax(_)), "{r}");
    }
    assert_eq!(check_rune("not base64!"), Err(R::RuneNotBase64));
    assert_eq!(check_rune(""), Err(R::RuneNotBase64));
    assert_eq!(
        check_rune(&base64(&[0; 31], Alphabet::Url)),
        Err(R::RuneNotBase64)
    );
    // Restrictions that are not text.
    let mut bytes = vec![0u8; 32];
    bytes.extend([0xff, 0xfe, 0xfd]);
    assert_eq!(
        check_rune(&base64(&bytes, Alphabet::Url)),
        Err(R::RuneNotText)
    );
    // The other alphabet of base64 is not a rune.
    let mut standard = vec![0xfb; 32];
    standard.extend_from_slice(b"method=invoice|method=listinvoices");
    let text = base64(&standard, Alphabet::Standard);
    assert!(text.contains('+') || text.contains('/'));
    assert_eq!(check_rune(&text), Err(R::RuneNotBase64));
}

#[test]
fn ln_08_the_rune_is_held_only_when_it_passes() {
    assert!(ClnCredential::new(endpoint(), &rune(INVOICE_RUNE)).is_ok());
    assert!(ClnCredential::new(endpoint(), &rune("method=pay")).is_err());
}

fn lndconnect(host: &str, cert: &[u8], mac: &[u8]) -> String {
    format!(
        "lndconnect://{host}?cert={}&macaroon={}",
        base64(cert, Alphabet::Url).trim_end_matches('='),
        base64(mac, Alphabet::Url).trim_end_matches('=')
    )
}

#[test]
fn ln_11_an_lndconnect_address_gives_the_node_the_pin_and_the_macaroon() {
    let cert = der(300, 7);
    let uri = lndconnect("umbrel.local:10009", &cert, &macaroon());
    let c = LndCredential::from_lndconnect(&uri).expect("reads");
    assert_eq!(c.endpoint().host(), "umbrel.local");
    assert_eq!(c.endpoint().port(), 10009);
    assert!(c.endpoint().pin().accepts(&cert));
    assert_eq!(c.header_value(), hex(&macaroon()));
    assert_eq!(c.endpoint().host_port(), "umbrel.local:10009");
    // An address of a node and a Tor name read too.
    for host in ["192.168.1.20:8080", "[fd00::1]:8080", "abcdefgh.onion:8080"] {
        assert!(LndCredential::from_lndconnect(&lndconnect(host, &cert, &macaroon())).is_ok());
    }
}

#[test]
fn ln_11_a_connect_string_that_is_not_exact_is_refused() {
    let cert = der(300, 7);
    let good = lndconnect("n.local:8080", &cert, &macaroon());
    let c = |s: &str| LndCredential::from_lndconnect(s).unwrap_err();
    assert_eq!(
        c("lnd://n.local:8080?cert=AA&macaroon=AA"),
        R::BadConnectString
    );
    assert_eq!(
        c(&good.replace("lndconnect://", "http://")),
        R::BadConnectString
    );
    assert_eq!(c("lndconnect://n.local:8080"), R::BadConnectString);
    assert_eq!(c(&format!("{good}&x=1")), R::BadConnectString);
    assert_eq!(c(&format!("{good}&cert=AA")), R::BadConnectString);
    assert_eq!(c(&format!("{good}&macaroon")), R::BadConnectString);
    // No certificate: nothing to pin.
    let mac = base64(&macaroon(), Alphabet::Url);
    assert_eq!(
        c(&format!("lndconnect://n.local:8080?macaroon={mac}")),
        R::NoCertificate
    );
    // No macaroon, a certificate that is not one, a macaroon that is not one.
    let cert64 = base64(&cert, Alphabet::Url);
    assert_eq!(
        c(&format!("lndconnect://n.local:8080?cert={cert64}")),
        R::BadMacaroon
    );
    assert_eq!(
        c(&lndconnect("n.local:8080", &[1, 2, 3], &macaroon())),
        R::BadCertificate
    );
    assert_eq!(
        c(&lndconnect("n.local:8080", &cert, &[9, 9])),
        R::BadMacaroon
    );
    // A certificate in the wrong alphabet.
    assert_eq!(
        c(&format!(
            "lndconnect://n.local:8080?cert=+++/&macaroon={mac}"
        )),
        R::BadCertificate
    );
    // Too long.
    assert_eq!(
        c(&format!("{good}{}", "a".repeat(9000))),
        R::BadConnectString
    );
}

#[test]
fn ln_11_the_address_of_a_node_is_a_host_and_a_port() {
    let p = pin();
    for ok in [
        "a.b-c.d:1",
        "node:65535",
        "1.2.3.4:8080",
        "[::1]:8080",
        "x.onion:80",
    ] {
        assert!(Endpoint::parse(ok, p.clone()).is_ok(), "{ok}");
    }
    for bad in [
        "node",
        "node:",
        ":8080",
        "node:0",
        "node:08",
        "node:65536",
        "node:80a",
        "no de:80",
        "-node:80",
        "node-:80",
        "a..b:80",
        "::1:8080",
        "[node]:80",
        "[]:80",
        "https://node:80",
        "node/x:80",
        "node:80/x",
    ] {
        assert_eq!(
            Endpoint::parse(bad, p.clone()).unwrap_err(),
            R::BadAddress,
            "{bad}"
        );
    }
    let long = format!("{}:80", "a".repeat(254));
    assert!(Endpoint::parse(&long, p.clone()).is_err());
    let label = format!("{}.com:80", "a".repeat(64));
    assert!(Endpoint::parse(&label, p).is_err());
}

#[test]
fn ln_12_only_the_pinned_certificate_is_accepted() {
    let cert = der(300, 7);
    let other = der(300, 8);
    let by_bytes = CertPin::from_der(&cert).unwrap();
    assert!(by_bytes.accepts(&cert));
    assert!(!by_bytes.accepts(&other));
    assert!(!by_bytes.accepts(&[]));
    let digest = sha256::Hash::hash(&cert).to_byte_array();
    let by_digest = CertPin::from_sha256_hex(&hex(&digest)).unwrap();
    assert!(by_digest.accepts(&cert));
    assert!(!by_digest.accepts(&other));
    assert_eq!(by_digest.sha256(), by_bytes.sha256());
    // As openssl writes a fingerprint, in capitals with colons.
    let colons: Vec<String> = digest.iter().map(|b| format!("{b:02X}")).collect();
    let by_openssl = CertPin::from_sha256_hex(&colons.join(":")).unwrap();
    assert_eq!(by_openssl, by_digest);
    for bad in [
        "",
        "ab",
        &hex(&digest[..31]),
        &format!("{}zz", hex(&digest[..31])),
    ] {
        assert_eq!(
            CertPin::from_sha256_hex(bad).unwrap_err(),
            R::BadFingerprint,
            "{bad}"
        );
    }
}

#[test]
fn ln_12_a_pem_file_as_the_node_writes_it_is_read() {
    let cert = der(300, 7);
    let b64 = base64(&cert, Alphabet::Standard);
    let lines: Vec<&str> = b64
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    let pem = format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        lines.join("\n")
    );
    assert_eq!(
        CertPin::from_pem(&pem).unwrap(),
        CertPin::from_der(&cert).unwrap()
    );
    // Two certificates, none, or one that is cut.
    assert!(CertPin::from_pem(&format!("{pem}{pem}")).is_err());
    assert!(CertPin::from_pem("").is_err());
    assert!(CertPin::from_pem(&pem.replace("-----END CERTIFICATE-----", "")).is_err());
    assert!(CertPin::from_pem(&pem.replace("AAAA", "!!!!").replace('M', "!")).is_err());
}

#[test]
fn ln_12_a_certificate_must_be_one_der_sequence() {
    assert!(CertPin::from_der(&der(300, 7)).is_ok());
    assert!(CertPin::from_der(&der(100, 7)).is_ok());
    assert_eq!(CertPin::from_der(&[]).unwrap_err(), R::BadCertificate);
    assert_eq!(
        CertPin::from_der(&[0x30; 100]).unwrap_err(),
        R::BadCertificate
    );
    let mut short = der(300, 7);
    short.pop();
    assert_eq!(CertPin::from_der(&short).unwrap_err(), R::BadCertificate);
    let mut long = der(300, 7);
    long.push(0);
    assert_eq!(CertPin::from_der(&long).unwrap_err(), R::BadCertificate);
    let mut tag = der(300, 7);
    tag[0] = 0x31;
    assert_eq!(CertPin::from_der(&tag).unwrap_err(), R::BadCertificate);
    assert!(CertPin::from_der(&der(9000, 1)).is_err());
}

#[test]
fn ln_23_a_credential_is_never_in_a_debug_text() {
    let secret_rune = rune(INVOICE_RUNE);
    let c = ClnCredential::new(endpoint(), &secret_rune).unwrap();
    assert!(!format!("{c:?}").contains(&secret_rune));
    let l = LndCredential::new(endpoint(), &macaroon()).unwrap();
    assert!(!format!("{l:?}").contains(&hex(&macaroon())));
    let ask = ask();
    for cred in [lnd(), cln()] {
        let text = format!("{:?}", cred.create_request(&ask));
        assert!(!text.contains(&hex(&macaroon())));
        assert!(!text.contains(&secret_rune));
        assert!(text.contains("<not shown>"));
    }
}

#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_lightning::bolt11::{check, decode, Expectation};
use openbmt_lightning::credential::check_rune;
use openbmt_lightning::{
    Ask, CertPin, ClnCredential, Credential, Endpoint, LndCredential, Payment, Reply,
};

/// Base64 of the URL alphabet with no padding, for the rune of the target.
fn base64url(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(char::from(T[(n >> (18 - 6 * i) & 63) as usize]));
        }
    }
    out
}

fuzz_target!(|data: &[u8]| {
    // The first byte says which reader gets the rest.
    let Some((&which, rest)) = data.split_first() else {
        return;
    };
    match which % 4 {
        // The invoice: read it, then check it against a sale.
        0 => {
            let Ok(text) = std::str::from_utf8(rest) else {
                return;
            };
            let first = decode(text);
            // The same text always gives the same answer.
            assert_eq!(first, decode(text));
            let Ok(ask) = Ask::sale([1, 2, 3, 4], 17, 0, 25_000_000, 300) else {
                return;
            };
            if let Ok(d) = first {
                // A text that is read is also read in lower case, to the
                // same invoice.
                assert_eq!(Ok(d.clone()), decode(&text.to_ascii_lowercase()));
                let expect = Expectation {
                    network: bitcoin::Network::Regtest,
                    node: d.signer,
                    payment_hash: d.payment_hash,
                    ask,
                };
                if let Ok(checked) = check(text, &expect) {
                    // What passes the check names the amount, the hash and
                    // the signer asked for.
                    assert_eq!(checked.amount_msat, 25_000_000);
                    assert_eq!(checked.payment_hash, d.payment_hash);
                    assert_eq!(decode(&checked.bolt11), Ok(d));
                }
            }
        }
        // The rune: read, and refused or taken, with no panic.
        1 => {
            if let Ok(text) = std::str::from_utf8(rest) {
                let first = check_rune(text);
                assert_eq!(first, check_rune(text));
            }
        }
        // The connect string.
        2 => {
            if let Ok(text) = std::str::from_utf8(rest) {
                if let Ok(c) = LndCredential::from_lndconnect(text) {
                    // Whatever it names, it is a pin and a host.
                    assert!(c.endpoint().port() != 0);
                    assert!(!c.endpoint().host().is_empty());
                }
            }
        }
        // The replies of the node: never paid unless the amount is there.
        _ => {
            let Some((&amount, body)) = rest.split_first() else {
                return;
            };
            let asked = u64::from(amount) * 1_000;
            let Ok(endpoint) = Endpoint::parse("n.local:8080", CertPin::Sha256([0; 32])) else {
                return;
            };
            let mut mac = vec![2u8];
            mac.extend_from_slice(&[7; 40]);
            let mut rune_bytes = vec![0xabu8; 32];
            rune_bytes.extend_from_slice(b"method=invoice|method=listinvoices");
            let (Ok(lnd), Ok(cln)) = (
                LndCredential::new(endpoint.clone(), &mac),
                ClnCredential::new(endpoint, &base64url(&rune_bytes)),
            ) else {
                return;
            };
            let reply = Reply {
                status: 200,
                body: body.to_vec(),
            };
            let hash = [9u8; 32];
            for c in [Credential::Lnd(lnd), Credential::Cln(cln)] {
                let first = c.parse_lookup(&reply, &hash, asked);
                assert_eq!(first, c.parse_lookup(&reply, &hash, asked));
                if let Ok(Payment::Paid(facts)) = first {
                    assert!(facts.amount_received_msat >= asked);
                    assert_eq!(facts.payment_hash, hash);
                }
                let _ = c.parse_create(&reply);
            }
        }
    }
});

#![no_main]

use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_lightning::connect::{ConnectionTest, Next};
use openbmt_lightning::{
    Ask, CertPin, ClnCredential, Credential, Endpoint, LndCredential, Reply,
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

fn credentials() -> Option<[Credential; 2]> {
    let endpoint = Endpoint::parse("n.local:8080", CertPin::Sha256([0; 32])).ok()?;
    let mut mac = vec![2u8];
    mac.extend_from_slice(&[7; 40]);
    let mut rune_bytes = vec![0xabu8; 32];
    rune_bytes.extend_from_slice(b"method=invoice|method=listinvoices");
    let lnd = LndCredential::new(endpoint.clone(), &mac).ok()?;
    let cln = ClnCredential::new(endpoint, &base64url(&rune_bytes)).ok()?;
    Some([Credential::Lnd(lnd), Credential::Cln(cln)])
}

fuzz_target!(|data: &[u8]| {
    // The first byte says which reader gets the rest.
    let Some((&which, rest)) = data.split_first() else {
        return;
    };
    match which % 3 {
        // The connection test, taking the same reply at each step: it ends
        // in at most three replies, never panics, and the same replies
        // always give the same end.
        0 => {
            let Some((&status, body)) = rest.split_first() else {
                return;
            };
            let Some(creds) = credentials() else {
                return;
            };
            let status = [200, 400, 401, 403, 404, 500, 900, 0][usize::from(status) % 8];
            let reply = Reply {
                status,
                body: body.to_vec(),
            };
            for credential in creds {
                let run = || {
                    let mut test =
                        ConnectionTest::new(credential.clone(), Network::Regtest, [2; 33], [1; 4], 5);
                    for _ in 0..4 {
                        let _ = test.request();
                        match test.accept(&reply) {
                            Ok(Next::More(next)) => test = next,
                            Ok(Next::Done(_)) => return Some(true),
                            Err(_) => return Some(false),
                        }
                    }
                    None
                };
                let first = run();
                // Three steps at most: more than that is a loop.
                assert!(first.is_some());
                assert_eq!(first, run());
            }
        }
        // The reply to a lookup by label.
        1 => {
            let Some((&status, body)) = rest.split_first() else {
                return;
            };
            let Some(creds) = credentials() else {
                return;
            };
            let Ok(ask) = Ask::sale([1, 2, 3, 4], 17, 0, 25_000_000, 300) else {
                return;
            };
            let status = [200, 400, 404, 500, 900][usize::from(status) % 5];
            let reply = Reply {
                status,
                body: body.to_vec(),
            };
            for credential in creds {
                let first = credential.parse_existing(&reply, &ask);
                assert_eq!(first, credential.parse_existing(&reply, &ask));
                let restored = credential.parse_restored(&reply, &ask);
                assert_eq!(restored, credential.parse_restored(&reply, &ask));
                if first.is_ok() {
                    assert_eq!(first, restored);
                }
            }
        }
        // The pin: a certificate that is read is the one that it accepts,
        // by its bytes and by its hash, and no other.
        _ => {
            if let Ok(pin) = CertPin::from_der(rest) {
                assert!(pin.accepts(rest));
                assert!(CertPin::Sha256(pin.sha256()).accepts(rest));
                let mut other = rest.to_vec();
                if let Some(last) = other.last_mut() {
                    *last ^= 1;
                    assert!(!pin.accepts(&other));
                    assert!(!CertPin::Sha256(pin.sha256()).accepts(&other));
                }
            }
            let pin = CertPin::Sha256([9; 32]);
            let _ = pin.accepts(rest);
        }
    }
});

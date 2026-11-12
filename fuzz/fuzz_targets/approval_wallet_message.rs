#![no_main]

use bitcoin::secp256k1::{Secp256k1, SecretKey};
use bitcoin::{Address, CompressedPublicKey, Network};
use libfuzzer_sys::fuzz_target;
use openbmt_core::approval::{verify_wallet_message, WalletSigError};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut s = String::new();
    for c in bytes.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, b)| a | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= c.len() {
                s.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                s.push('=');
            }
        }
    }
    s
}

// The check on arbitrary text and arbitrary signature bytes never panics.
// The first byte picks the address (a P2WPKH one, or one of the other types
// that must be refused) and whether the rest is read as signature text or
// is written as base64 first, so that the compact and BIP-322 paths are
// reached. The second byte is the length of the message.
fuzz_target!(|data: &[u8]| {
    let [pick, mlen, rest @ ..] = data else { return };
    let secp = Secp256k1::new();
    let Ok(sk) = SecretKey::from_slice(&[(*pick >> 4).max(1); 32]) else {
        return;
    };
    let pk = CompressedPublicKey(sk.public_key(&secp));
    let address = match *pick & 3 {
        0 | 1 => Address::p2wpkh(&pk, Network::Bitcoin),
        2 => Address::p2pkh(pk, Network::Bitcoin),
        _ => Address::p2wpkh(&pk, Network::Testnet),
    };
    let split = (usize::from(*mlen)).min(rest.len());
    let (message, sig) = rest.split_at(split);
    let message = String::from_utf8_lossy(message);
    let text = if *pick & 4 == 0 {
        String::from_utf8_lossy(sig).into_owned()
    } else {
        base64(sig)
    };
    let result = verify_wallet_message(&address, &message, &text);
    if *pick & 3 >= 2 {
        assert_eq!(result, Err(WalletSigError::UnsupportedAddress));
    }
    // Arbitrary bytes are not the signature of a key they were not made
    // with: a pass here would be a forgery.
    assert!(result.is_err());
});

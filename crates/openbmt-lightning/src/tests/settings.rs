//! LN-03: the node's key is part of the shop's signed settings. The
//! settings' own tests are in `openbmt-core` (`ln_03_` in `settings.rs`) and
//! the Key app's line in `openbmt-approver`; this one ties the key of the
//! settings to the check of an invoice.

use openbmt_core::settings::{classify_change, Change, Settings};

use super::*;
use crate::bolt11::{check, Expectation};
use crate::error::InvoiceRefused;

#[test]
fn ln_03_the_signer_of_an_invoice_is_compared_with_the_key_of_the_settings() {
    let mut s = Settings::defaults(1_000);
    s.lightning_node = Some(node_key(1));
    assert_eq!(s.validate(), Ok(()));
    let expect = |node: [u8; 33]| Expectation {
        network: NETWORK,
        node,
        payment_hash: hash(1),
        ask: ask(),
    };
    let text = Make::good(hash(1)).write();
    // The key of the settings is the one the check takes, byte for byte.
    assert!(check(&text, &expect(s.lightning_node.unwrap())).is_ok());
    // Settings that name another node refuse the invoice.
    let mut other = s.clone();
    other.lightning_node = Some(node_key(2));
    assert_eq!(
        check(&text, &expect(other.lightning_node.unwrap())),
        Err(InvoiceRefused::WrongNode)
    );
}

#[test]
fn ln_03_changing_the_node_in_the_settings_loosens() {
    let mut a = Settings::defaults(1_000);
    let mut b = a.clone();
    b.lightning_node = Some(node_key(1));
    assert_eq!(classify_change(&a, &b), Change::Loosening);
    assert_eq!(classify_change(&b, &a), Change::Tightening);
    a.lightning_node = Some(node_key(2));
    assert_eq!(classify_change(&a, &b), Change::Loosening);
    // The credential is not in the settings: they hold no secret of the
    // counter, only the public key of the node.
    let bytes = b.encode();
    let macaroon = macaroon();
    assert!(!bytes.windows(macaroon.len()).any(|w| w == macaroon));
    assert!(bytes.windows(33).any(|w| w == node_key(1)));
}

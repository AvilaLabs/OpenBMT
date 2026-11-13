#![no_main]

use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, Txid};
use libfuzzer_sys::fuzz_target;
use openbmt_owner_node::{Owed, OwedKind, SettleOwed};

// Any bytes are read strictly or refused, and what is read gives the same
// bytes back (OC-52). The first byte picks whether the rest follows the start
// of a valid file of three items, so that mutated bytes get past the header
// and the items' checks.
fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let bytes: Vec<u8> = if mode & 1 == 1 {
        let item = |kind: OwedKind, n: u8| Owed {
            kind,
            txid: Txid::from_byte_array([n; 32]),
            block: BlockHash::from_byte_array([n.wrapping_add(1); 32]),
            tx: None,
            // OC-55: the beaten kinds carry the transaction that beat them.
            by: (!kind.paid()).then(|| Txid::from_byte_array([n.wrapping_add(2); 32])),
        };
        let valid = SettleOwed::new()
            .merged(&[
                item(OwedKind::ExportedPaid, 1),
                item(OwedKind::ElsewherePaid, 2),
                item(OwedKind::ElsewhereBeaten, 3),
            ])
            .map(|o| o.to_bytes())
            .unwrap_or_default();
        let keep = usize::from(mode >> 1).min(valid.len());
        let mut v = valid[..keep].to_vec();
        v.extend_from_slice(rest);
        v
    } else {
        rest.to_vec()
    };
    if let Ok(owed) = SettleOwed::from_bytes(&bytes) {
        assert_eq!(owed.to_bytes(), bytes);
    }
});

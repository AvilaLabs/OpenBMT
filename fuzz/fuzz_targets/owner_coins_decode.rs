#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_owner_node::coins::CoinView;

// Any bytes are read strictly or refused, and what is read gives the same
// bytes back (OC-08). The first byte picks whether the rest follows the
// start of a valid empty view, so that mutated bytes get past the header.
fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let bytes: Vec<u8> = if mode & 1 == 1 {
        let valid = CoinView::new(u32::from(mode)).to_bytes();
        let keep = usize::from(mode >> 1).min(valid.len());
        let mut v = valid[..keep].to_vec();
        v.extend_from_slice(rest);
        v
    } else {
        rest.to_vec()
    };
    if let Ok(view) = CoinView::from_bytes(&bytes) {
        assert_eq!(view.to_bytes(), bytes);
    }
});
